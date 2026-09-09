use crate::action::{Action, ActionExt, Actions, NullActionExt};
use crate::binds::{BindMap, BindMapExt, SimpleMouseEvent, Trigger, TriggerKind};
use crate::message::{BindDirective, Event, RenderCommand};
use anyhow::Result;
use cba::bait::ResultExt;
use cba::bath::PathExt;
use cba::unwrap;
use crokey::{Combiner, KeyCombination, KeyCombinationFormat, key};
use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyModifiers, MouseEvent, MouseEventKind,
};
use futures::stream::StreamExt;
use log::{debug, error, info, warn};
use ratatui::layout::Rect;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{self};

pub type RenderSender<A = NullActionExt> = mpsc::UnboundedSender<RenderCommand<A>>;
pub type EventSender = mpsc::UnboundedSender<Event>;
pub type BindSender<A> = mpsc::UnboundedSender<BindDirective<A>>;

#[derive(Debug)]
pub struct EventLoop<A: ActionExt> {
    txs: Vec<mpsc::UnboundedSender<RenderCommand<A>>>,
    tick_interval: time::Duration,

    pub binds: BindMap<A>,
    combiner: Combiner,
    fmt: KeyCombinationFormat,

    mouse_events: bool,
    paused: bool,
    event_stream: Option<EventStream>,

    rx: mpsc::UnboundedReceiver<Event>,
    controller_tx: mpsc::UnboundedSender<Event>,

    bind_rx: mpsc::UnboundedReceiver<BindDirective<A>>,
    bind_tx: BindSender<A>,

    key_file: Option<PathBuf>,
    current_task: Option<tokio::task::JoinHandle<Result<()>>>,
}

impl<A: ActionExt> Default for EventLoop<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: ActionExt> EventLoop<A> {
    pub fn new() -> Self {
        let combiner = Combiner::default();
        let fmt = KeyCombinationFormat::default();
        let (controller_tx, controller_rx) = tokio::sync::mpsc::unbounded_channel();

        let (bind_tx, bind_rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            txs: vec![],
            tick_interval: time::Duration::from_millis(200),

            binds: BindMap::new(),
            combiner,
            fmt,
            event_stream: None, // important not to initialize it too early?
            rx: controller_rx,
            controller_tx,

            mouse_events: false,
            paused: false,
            key_file: None,
            current_task: None,

            bind_rx,
            bind_tx,
        }
    }

    pub fn with_binds(mut binds: BindMap<A>) -> Self {
        let mut ret = Self::new();
        binds.resolve_semantics();
        ret.binds = binds;
        ret
    }

    pub fn check_binds(&self) -> Result<(), String> {
        self.binds.check_cycles()
    }

    pub fn record_last_key(&mut self, path: PathBuf) -> &mut Self {
        self.key_file = Some(path);
        self
    }

    pub fn with_tick_rate(mut self, tick_rate: u8) -> Self {
        self.tick_interval = time::Duration::from_secs_f64(1.0 / tick_rate as f64);
        self
    }

    pub fn add_tx(&mut self, handler: mpsc::UnboundedSender<RenderCommand<A>>) -> &mut Self {
        self.txs.push(handler);
        self
    }

    pub fn with_mouse_events(mut self) -> Self {
        self.mouse_events = true;
        self
    }

    pub fn clear_txs(&mut self) {
        self.txs.clear();
    }

    pub fn controller(&self) -> EventSender {
        self.controller_tx.clone()
    }
    pub fn bind_controller(&self) -> BindSender<A> {
        self.bind_tx.clone()
    }

    fn get_bind(&self, kind: TriggerKind) -> Option<Actions<A>> {
        let mode = crate::MODE.lock().ok()?.clone();
        self.binds
            .get(&Trigger {
                kind: kind.clone(),
                mode: mode.clone(),
            })
            .or_else(|| {
                (!mode.is_empty()).then(|| {
                    self.binds.get(&Trigger {
                        kind,
                        mode: String::new(),
                    })
                })?
            })
            .cloned()
    }

    fn dispatch_fallback_key(&mut self, key: KeyCombination) {
        let key_str = key.to_string();
        let mut matched = true;
        // a basic set of keys to ensure basic usability
        match key {
            key!(ctrl - c) => {
                self.send(RenderCommand::quit_with(130));
            }
            key!(esc) => {
                self.send_key_action(Action::Quit(1), key_str);
            }
            key!(up) => self.send_key_action(Action::Up(1), key_str),
            key!(down) => self.send_key_action(Action::Down(1), key_str),
            key!(enter) => self.send_key_action(Action::Accept, key_str),
            key!(right) => self.send_key_action(Action::ForwardChar, key_str),
            key!(left) => self.send_key_action(Action::BackwardChar, key_str),
            key!(ctrl - right) => self.send_key_action(Action::ForwardWord, key_str),
            key!(ctrl - left) => self.send_key_action(Action::BackwardWord, key_str),
            key!(backspace) => self.send_key_action(Action::DeleteChar, key_str),
            key!(ctrl - h) => self.send_key_action(Action::DeleteWord, key_str),
            key!(ctrl - u) => self.send_key_action(Action::Cancel, key_str),
            key!(alt - h) => self.send_key_action(Action::Help("".to_string()), key_str),
            key!(ctrl - '[') => self.send_key_action(Action::ToggleWrap, key_str),
            key!(ctrl - ']') => self.send_key_action(Action::TogglePreviewWrap, key_str),
            _ => {
                matched = false;
            }
        }
        if matched {
            self.record_key(key.to_string());
        }
    }

    fn handle_event(&mut self, e: Event) {
        debug!("Received: {e}");

        match e {
            Event::Pause => {
                self.paused = true;
                self.send(RenderCommand::Ack);
                self.event_stream = None; // drop because EventStream "buffers" event
            }
            Event::Refresh => {
                self.send(RenderCommand::Refresh);
            }
            _ => {}
        }
        if let Some(actions) = self.get_bind(TriggerKind::Event(e)) {
            self.send_actions(actions, None);
        }
    }

    fn handle_rebind(&mut self, e: BindDirective<A>) {
        debug!("Received: {e:?}");

        match e {
            BindDirective::Bind(k, v) => {
                self.binds.insert(k, v);
            }

            BindDirective::PushBind(k, v) => {
                self.binds.entry(k).or_default().0.push(v);
            }

            BindDirective::Unbind(k) => {
                self.binds.shift_remove(&k);
            }

            BindDirective::PopBind(k) => {
                if let Some(actions) = self.binds.get_mut(&k) {
                    actions.0.pop();

                    if actions.0.is_empty() {
                        self.binds.shift_remove(&k);
                    }
                }
            }

            BindDirective::Action(action) => {
                self.send_actions(vec![action], None);
            }
        }
    }

    pub fn binds(&mut self, binds: BindMap<A>) -> &mut Self {
        self.binds = binds;
        self
    }

    // todo: should its return type carry info
    pub async fn run(&mut self) {
        log::trace!("{:?}", self.binds);
        self.event_stream = Some(EventStream::new());
        let mut interval = time::interval(self.tick_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        let mut sig = termination_status();

        if let Some(path) = self.key_file.clone() {
            log::debug!("Cleaning up temp files @ {path:?}");
            tokio::spawn(async move {
                cleanup_tmp_files(&path).await._elog();
            });
        }

        // this loops infinitely until all readers are closed
        loop {
            self.txs.retain(|tx| !tx.is_closed());
            if self.txs.is_empty() {
                log::trace!("Event loop completed");
                break;
            }

            // wait for resume signal
            while self.paused {
                if let Some(event) = self.rx.recv().await {
                    if matches!(event, Event::Resume) {
                        debug!("Resumed from pause");
                        self.paused = false;
                        self.send(RenderCommand::Ack);
                        self.event_stream = Some(EventStream::new());
                        break;
                    }
                } else {
                    error!("Event controller closed while paused.");
                    break;
                }
            }

            // // flush controller events
            // while let Ok(event) = self.rx.try_recv() {
            //    self.handle_event(event)
            // }

            let event = if let Some(stream) = &mut self.event_stream {
                stream.next()
            } else {
                continue; // event stream is removed when paused by handle_event
            };

            tokio::select! {
                _ = interval.tick() => {
                    self.send(RenderCommand::Tick)
                }

                // Termination signals (SIGINT, SIGTERM, SIGHUP)
                code = &mut sig => {
                    info!("Received termination signal with code {code}");
                    self.send(RenderCommand::quit_with(code));
                    sig = termination_status();
                }

                Some(event) = self.rx.recv() => {
                    self.handle_event(event)
                }

                Some(directive) = self.bind_rx.recv() => {
                    self.handle_rebind(directive)
                }

                // Input ready
                maybe_event = event => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            if !matches!(
                                event,
                                CrosstermEvent::Mouse(MouseEvent {
                                    kind: crossterm::event::MouseEventKind::Moved,
                                    ..
                                }) |  CrosstermEvent::Key {..}
                            ) {
                                info!("Event {event:?}");
                            }
                            match event {
                                CrosstermEvent::Key(k) => {
                                        if let Some(key) = self.combiner.transform(k) {
                                        info!("{key:?}");
                                        let key = KeyCombination::normalized(key);
                                        // When the action box is active, plain character keys
                                        // must always reach the input as Action::Char regardless
                                        // of any shortcut bindings on that key.
                                        let action_box_active = crate::ACTION_BOX_ACTIVE
                                            .load(std::sync::atomic::Ordering::Relaxed);
                                        if !action_box_active {
                                            if let Some(actions) = self.get_bind(TriggerKind::Key(key)) {
                                                self.record_key(key.to_string());
                                                self.send_actions(actions, Some(key.to_string()));
                                            } else if let Some(c) = key_code_as_letter(key) {
                                                self.send(RenderCommand::KeyAction {
                                                    key: c.to_string(),
                                                    action: Action::Char(c),
                                                });
                                            } else {
                                                self.dispatch_fallback_key(key);
                                            }
                                        } else if let Some(c) = key_code_as_letter(key) {
                                            // Action box is active: plain chars always go to input.
                                            // Non-char keys (ctrl-c, esc, enter, editing keys) still
                                            // go through the normal bind/fallback path.
                                            self.send(RenderCommand::KeyAction {
                                                key: c.to_string(),
                                                action: Action::Char(c),
                                            });
                                        } else if let Some(actions) = self.get_bind(TriggerKind::Key(key)) {
                                            self.record_key(key.to_string());
                                            self.send_actions(actions, Some(key.to_string()));
                                        } else {
                                            self.dispatch_fallback_key(key);
                                        }
                                    }
                                }
                                CrosstermEvent::Mouse(mouse) => {
                                    if let Some(actions) = self.get_bind(TriggerKind::Mouse(SimpleMouseEvent {
                                        kind: mouse.kind,
                                        modifiers: mouse.modifiers,
                                    })) {
                                        self.send_actions(actions, None);
                                    } else if !matches!(mouse.kind, MouseEventKind::Moved) {
                                        // mouse binds can be disabled by overriding with empty action
                                        // preview scroll can be disabled by overriding scroll event with scroll action
                                        self.send(RenderCommand::Mouse(mouse));
                                    }
                                }
                                CrosstermEvent::Resize(width, height) => {
                                    self.send(RenderCommand::Resize(Rect::new(0, 0, width, height)));
                                }
                                #[allow(unused_variables)]
                                CrosstermEvent::Paste(content) => {
                                    #[cfg(feature = "bracketed-paste")]
                                    {
                                        self.send(RenderCommand::Paste(content));
                                    }
                                    #[cfg(not(feature = "bracketed-paste"))]
                                    {
                                        unreachable!()
                                    }
                                }
                                // CrosstermEvent::FocusLost => {
                                // }
                                // CrosstermEvent::FocusGained => {
                                // }
                                _ => {},
                            }
                        }
                        Some(Err(e)) => warn!("Failed to read crossterm event: {e}"),
                        None => {
                            warn!("Reader closed");
                            break
                        }
                    }
                }
            }
        }
    }

    fn send(&self, action: RenderCommand<A>) {
        for tx in &self.txs {
            tx.send(action.clone())
                .unwrap_or_else(|_| debug!("Failed to send {action}"));
        }
    }

    fn record_key(&mut self, content: String) {
        let Some(path) = self.key_file.clone() else {
            return;
        };

        // Cancel previous task if still running
        if let Some(handle) = self.current_task.take() {
            handle.abort();
        }

        let handle = tokio::spawn(write_to_file(path, content));

        self.current_task = Some(handle);
    }

    fn send_actions<'a>(&self, actions: impl IntoIterator<Item = Action<A>>, key: Option<String>) {
        for action in actions {
            match action {
                Action::PrintKey => {
                    if let Some(k) = &key {
                        self.send(Action::Print(k.clone()).into());
                    }
                }
                Action::Semantic(s) => {
                    if let Some(actions) = self.get_bind(TriggerKind::Semantic(s.clone())) {
                        self.send_actions(actions.clone(), key.clone());
                    } else if let Some(k) = &key {
                        self.send(RenderCommand::KeyAction {
                            key: k.clone(),
                            action: Action::Semantic(s),
                        });
                    } else {
                        self.send(RenderCommand::Action(Action::Semantic(s)));
                    }
                }
                Action::SetMode(m) => {
                    if let Ok(mut mode) = crate::MODE.lock() {
                        *mode = m;
                    }
                }
                Action::Trace(t) => {
                    self.send(RenderCommand::Action(Action::Trace(t)));
                }
                _ => {
                    if let Some(k) = &key {
                        self.send(RenderCommand::KeyAction {
                            key: k.clone(),
                            action,
                        });
                    } else {
                        self.send(action.into());
                    }
                }
            }
        }
    }

    pub fn print_key(&self, key_combination: KeyCombination) -> String {
        self.fmt.to_string(key_combination)
    }

    fn send_key_action(&self, action: Action<A>, key: String) {
        self.send(RenderCommand::KeyAction { key, action });
    }

    #[allow(dead_code)]
    fn send_action(&self, action: Action<A>) {
        self.send(RenderCommand::Action(action));
    }
}

fn key_code_as_letter(key: KeyCombination) -> Option<char> {
    match key {
        KeyCombination {
            codes: crokey::OneToThree::One(crossterm::event::KeyCode::Char(l)),
            modifiers: KeyModifiers::NONE,
        } => Some(l),
        KeyCombination {
            codes: crokey::OneToThree::One(crossterm::event::KeyCode::Char(l)),
            modifiers: KeyModifiers::SHIFT,
        } => Some(l.to_ascii_uppercase()),
        _ => None,
    }
}

use std::path::Path;
use tokio::fs;

/// Cleanup files in the same directory with the same basename, and a .tmp extension
async fn cleanup_tmp_files(path: &Path) -> Result<()> {
    let parent = unwrap!(path.parent(); Ok(()));
    let name = unwrap!(path.file_name().and_then(|s| s.to_str()); Ok(()));

    let mut entries = fs::read_dir(parent).await?;

    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();

        if let Ok(filename) = entry_path.filename()
            && let Some(e) = filename.strip_prefix(name)
            && e.starts_with('.')
            && e.ends_with(".tmp")
        {
            fs::remove_file(entry_path).await._elog();
        }
    }

    Ok(())
}

/// Spawns a thread that writes `content` to `path` atomically using a temp file.
/// Returns the `JoinHandle` so you can wait for it if desired.
pub async fn write_to_file(path: PathBuf, content: String) -> Result<()> {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let tmp_path = path.with_file_name(format!("{}.{}.tmp", path.filename()?, suffix));

    // Write temp file
    fs::write(&tmp_path, &content).await?;

    // Atomically replace target
    fs::rename(&tmp_path, &path).await?;

    Ok(())
}

/// Resolves with the shell-conventional exit status of the first termination
/// signal received (SIGINT → 130, SIGTERM → 143, SIGHUP → 129). Pends forever
/// on platforms without signals or when listener registration fails.
fn termination_status() -> std::pin::Pin<Box<dyn std::future::Future<Output = i32> + Send>> {
    #[cfg(unix)]
    {
        use futures::FutureExt;
        use tokio::signal::unix::{SignalKind, signal};

        let listen = |kind: SignalKind, code: i32| match signal(kind) {
            Ok(mut rx) => async move {
                rx.recv().await;
                code
            }
            .boxed(),
            Err(e) => {
                error!("Failed to register signal listener: {e}");
                async move {
                    std::future::pending::<()>().await;
                    code
                }
                .boxed()
            }
        };

        let mut sigint = listen(SignalKind::interrupt(), 130);
        let mut sigterm = listen(SignalKind::terminate(), 143);
        let mut sighup = listen(SignalKind::hangup(), 129);

        // Fused: the render loop keeps polling this arm until teardown
        // finishes, and polling a completed future again would panic.
        Box::pin(
            async move {
                tokio::select! {
                    code = &mut sigint => code,
                    code = &mut sigterm => code,
                    code = &mut sighup => code,
                }
            }
            .fuse(),
        )
    }

    #[cfg(not(unix))]
    {
        Box::pin(std::future::pending())
    }
}
