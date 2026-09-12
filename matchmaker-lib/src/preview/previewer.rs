use ansi_to_tui::IntoText;
use cba::broc::{CommandExt, EnvVars};
use futures::FutureExt;
use log::{debug, error, warn};
use ratatui::text::{Line, Text};
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::watch::{Receiver, Sender, channel};
use tokio::task::JoinHandle;

use super::AppendOnly;
use crate::config::PreviewerConfig;
use crate::event::EventSender;
use crate::message::Event;
use crate::preview::Preview;

#[derive(Debug, Default, strum_macros::Display, Clone)]
pub enum PreviewMessage {
    Run(String, EnvVars),
    Set(Text<'static>),
    Media(String),
    Markdown(String, Option<usize>),
    Unset,
    #[default]
    Stop,
    Pause,
    Unpause,
}

#[derive(Debug)]
pub struct Previewer {
    /// The reciever for for [`PreviewMessage`]'s.
    rx: Receiver<PreviewMessage>,
    /// storage for preview command output
    lines: AppendOnly<Line<'static>>,
    /// storage for preview string override
    string: Arc<Mutex<Option<Text<'static>>>>,
    /// storage for preview image override
    image: Arc<Mutex<Option<image::DynamicImage>>>,
    /// Version ID for image state to detect changes reliably
    pub image_id: Arc<AtomicU64>,
    /// Flag which is set to true whenever the state changes
    /// and which the viewer can toggle after receiving the current state
    changed: Arc<AtomicBool>,

    /// Rendered line offsets of Mermaid diagram blocks within the current Markdown preview.
    /// Each entry is the 0-based line index in the rendered Text where a diagram starts.
    /// Shared with `Preview` so the render loop can navigate between diagrams.
    pub diagram_offsets: Arc<Mutex<Vec<usize>>>,
    /// Extracted Mermaid diagram source strings for the current Markdown file.
    pub diagram_sources: Arc<Mutex<Vec<String>>>,
    /// Index of the currently displayed diagram.
    pub current_diagram_idx: Arc<std::sync::atomic::AtomicUsize>,

    paused: bool,
    /// Maintain a queue of child processes to improve cleanup reliability
    procs: Vec<Child>,
    /// The currently executing child process
    current: Option<(Child, JoinHandle<bool>)>,

    pub config: PreviewerConfig,
    last: String,
    /// Event loop controller
    // We only use it to send [`ControlEvent::Event`]
    event_controller_tx: Option<EventSender>,
}

#[inline]
fn try_parse_simple_command(cmd: &str) -> Option<Command> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    // If command contains any shell metacharacter, let the shell handle it
    if trimmed.chars().any(|c| {
        matches!(
            c,
            '|' | '>'
                | '<'
                | '&'
                | ';'
                | '$'
                | '`'
                | '\n'
                | '*'
                | '?'
                | '('
                | ')'
                | '{'
                | '}'
                | '\''
                | '"'
                | '\\'
        )
    }) {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let prog = parts.next()?;
    let mut command = Command::new(prog);
    for arg in parts {
        command.arg(arg);
    }
    Some(command)
}

impl Previewer {
    pub fn new(config: PreviewerConfig) -> (Self, Sender<PreviewMessage>) {
        let (tx, rx) = channel(PreviewMessage::Stop);

        let new = Self {
            rx,
            lines: AppendOnly::new(),
            string: Default::default(),
            image: Default::default(),
            image_id: Arc::new(AtomicU64::new(1)),
            changed: Default::default(),
            diagram_offsets: Default::default(),
            diagram_sources: Default::default(),
            current_diagram_idx: Default::default(),
            paused: false,

            procs: Vec::new(),
            current: None,
            config,
            last: Default::default(),
            event_controller_tx: None,
        };

        (new, tx)
    }

    pub fn view(&self) -> Preview {
        Preview::new(
            self.lines.clone(),
            self.string.clone(),
            self.image.clone(),
            self.image_id.clone(),
            self.changed.clone(),
            self.diagram_offsets.clone(),
            self.diagram_sources.clone(),
            self.current_diagram_idx.clone(),
        )
    }

    pub fn set_string(&self, s: Text<'static>) {
        if let Ok(mut guard) = self.string.lock() {
            *guard = Some(s);
            self.changed.store(true, Ordering::Release);
        }
    }

    pub fn clear_string(&self) {
        if let Ok(mut guard) = self.string.lock() {
            *guard = None;
            self.changed.store(true, Ordering::Release);
        }
    }

    pub fn has_string(&self) -> bool {
        let guard = self.string.lock();
        guard.is_ok_and(|s| s.is_some())
    }

    pub fn set_image(&self, img: image::DynamicImage) {
        if let Ok(mut guard) = self.image.lock() {
            *guard = Some(img);
            self.image_id.fetch_add(1, Ordering::Release);
            self.changed.store(true, Ordering::Release);
        }
    }

    pub fn clear_image(&self) {
        if let Ok(mut guard) = self.image.lock() {
            if guard.is_some() {
                *guard = None;
                self.image_id.fetch_add(1, Ordering::Release);
                self.changed.store(true, Ordering::Release);
            }
        }
    }

    pub async fn run(mut self) -> Result<(), Vec<Child>> {
        while self.rx.changed().await.is_ok() {
            let m = self.rx.borrow_and_update().clone();

            let mut is_debouncable = false;
            let mut key = String::new();
            if let PreviewMessage::Run(cmd, _) = &m {
                is_debouncable = true;
                key = cmd.clone();
            } else if let PreviewMessage::Media(cmd) = &m {
                is_debouncable = true;
                key = cmd.clone();
            } else if let PreviewMessage::Markdown(cmd, width) = &m {
                is_debouncable = true;
                key = format!("{cmd}:{width:?}");
            }

            if is_debouncable {
                if !self.config.always_trigger && self.last == key {
                    continue;
                }

                if !self.config.delay_clear {
                    self.clear_image();
                    self.clear_string();
                    self.lines.clear();
                    self.changed.store(true, Ordering::Release);
                }

                if self.config.debounce_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(self.config.debounce_ms)).await;
                }

                while self.procs.len() >= self.config.max_procs {
                    self.prune_procs();
                    if self.procs.len() < self.config.max_procs {
                        break;
                    }

                    log::error!(
                        "too many zombie processes ({}), delaying preview update ({m:?}).",
                        self.procs.len()
                    );
                    let sleep = self.config.debounce_ms.max(10);
                    tokio::time::sleep(Duration::from_millis(sleep)).await;

                    if self.rx.has_changed().unwrap_or(false) {
                        break;
                    }
                }

                if self.rx.has_changed().unwrap_or(false) {
                    continue;
                }
            }

            if !self.procs.is_empty() {
                debug!("procs: {:?}", self.procs);
            }

            {
                match m {
                    PreviewMessage::Pause => {
                        self.paused = true;
                        continue;
                    }
                    PreviewMessage::Unpause => {
                        self.paused = false;
                        continue;
                    }
                    _ if self.paused => {
                        continue;
                    }
                    PreviewMessage::Set(s) => {
                        self.set_string(s.clone());
                        self.last.clear();
                        // don't kill the underlying
                        continue;
                    }
                    PreviewMessage::Unset => {
                        self.clear_string();
                        self.last.clear();
                        continue;
                    }
                    PreviewMessage::Media(ref path) => {
                        if !self.config.delay_clear {
                            self.clear_image();
                            self.clear_string();
                            self.lines.clear();
                        }
                        self.dispatch_kill();
                        self.last = path.clone();
                        let path = path.clone();
                        let image_state = self.image.clone();
                        let string_state = self.string.clone();
                        let image_id = self.image_id.clone();
                        let changed = self.changed.clone();
                        let event_tx = self.event_controller_tx.clone();

                        let media_size = self.config.media_size;
                        let rx = self.rx.clone();
                        tokio::task::spawn_blocking(move || {
                            if rx.has_changed().unwrap_or(false) {
                                return;
                            }

                            let img_result = if path.to_lowercase().ends_with(".pdf") {
                                // PDF support using pdftoppm if available
                                let pdf_scale = if media_size > 0 {
                                    media_size.to_string()
                                } else {
                                    "1200".to_string()
                                };
                                let output = std::process::Command::new("pdftoppm")
                                    .args([
                                        "-jpeg",
                                        "-r",
                                        "150",
                                        "-scale-to",
                                        &pdf_scale,
                                        "-f",
                                        "1",
                                        "-l",
                                        "1",
                                        &path,
                                    ])
                                    .output();
                                if let Ok(_out) = output {
                                    let p = std::path::PathBuf::from(&path);
                                    let stem = p
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("document");
                                    let rendered = p.with_file_name(format!("{}-1.jpg", stem));
                                    if rendered.exists() {
                                        let img = image::open(&rendered).ok();
                                        let _ = std::fs::remove_file(&rendered);
                                        img
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else if path.to_lowercase().ends_with(".mp4")
                                || path.to_lowercase().ends_with(".mkv")
                                || path.to_lowercase().ends_with(".webm")
                                || path.to_lowercase().ends_with(".mov")
                                || path.to_lowercase().ends_with(".avi")
                                || path.to_lowercase().ends_with(".flv")
                                || path.to_lowercase().ends_with(".m4v")
                                || path.to_lowercase().ends_with(".wmv")
                            {
                                // Video support
                                let media_size_str = if media_size > 0 {
                                    media_size.to_string()
                                } else {
                                    "512".to_string()
                                };
                                let output = std::process::Command::new("ffmpegthumbnailer")
                                    .args([
                                        "-i",
                                        &path,
                                        "-s",
                                        &media_size_str,
                                        "-t",
                                        "00:00:01",
                                        "-c",
                                        "jpeg",
                                        "-q",
                                        "10",
                                        "-o",
                                        "-",
                                    ])
                                    .output();
                                if let Ok(out) = output {
                                    // ffmpegthumbnailer sometimes pollutes stdout with debug logs.
                                    // We search for the JPEG start of image marker (FF D8 FF) and slice the buffer.
                                    let data = &out.stdout;
                                    let img_data = if let Some(pos) =
                                        data.windows(3).position(|w| w == &[0xff, 0xd8, 0xff])
                                    {
                                        &data[pos..]
                                    } else {
                                        data
                                    };
                                    image::load_from_memory(img_data).ok()
                                } else {
                                    None
                                }
                            } else {
                                let img = image::open(&path).ok();
                                if rx.has_changed().unwrap_or(false) {
                                    return;
                                }
                                img.map(|i| {
                                    let max_dim = media_size;
                                    if max_dim > 0 && (i.width() > max_dim || i.height() > max_dim)
                                    {
                                        i.thumbnail(max_dim, max_dim)
                                    } else {
                                        i
                                    }
                                })
                            };

                            if rx.has_changed().unwrap_or(false) {
                                return;
                            }

                            if let Some(img) = img_result {
                                if let Ok(mut guard) = image_state.lock() {
                                    *guard = Some(img);
                                    image_id.fetch_add(1, Ordering::Release);
                                    changed.store(true, Ordering::Release);
                                }
                                if let Ok(mut str_guard) = string_state.lock() {
                                    *str_guard = None;
                                }
                                if let Some(ref tx) = event_tx {
                                    let _ = tx.send(crate::message::Event::PreviewChange);
                                }
                            }
                        });
                        continue;
                    }
                    PreviewMessage::Markdown(ref path, width) => {
                        if !self.config.delay_clear {
                            self.clear_image();
                            self.clear_string();
                            self.lines.clear();
                        }
                        self.dispatch_kill();
                        // Clear any stale diagram offsets from previous preview
                        if let Ok(mut offsets) = self.diagram_offsets.lock() {
                            offsets.clear();
                        }
                        if let Ok(mut sources) = self.diagram_sources.lock() {
                            sources.clear();
                        }
                        self.current_diagram_idx
                            .store(0, std::sync::atomic::Ordering::Release);
                        self.last = format!("{path}:{width:?}");
                        let path = path.clone();
                        let string_state = self.string.clone();
                        let image_state = self.image.clone();
                        let image_id = self.image_id.clone();
                        let diagram_offsets_state = self.diagram_offsets.clone();
                        let diagram_sources_state = self.diagram_sources.clone();
                        let current_diagram_idx_state = self.current_diagram_idx.clone();
                        let changed = self.changed.clone();
                        let rx = self.rx.clone();
                        let event_tx = self.event_controller_tx.clone();
                        let media_enabled = self.config.media;
                        let markdown_diagrams = self.config.markdown_diagrams;

                        tokio::task::spawn_blocking(move || {
                            if rx.has_changed().unwrap_or(false) {
                                return;
                            }

                            let p = std::path::Path::new(&path);
                            let is_mermaid =
                                p.extension().and_then(|e| e.to_str()).is_some_and(|ext| {
                                    matches!(ext.to_lowercase().as_str(), "mmd" | "mermaid")
                                });

                            if is_mermaid && media_enabled && markdown_diagrams {
                                // Try graphics-based rendering first (Kitty protocol)
                                let img = crate::utils::mermaid::render_mermaid_file_to_image(
                                    p, 2.0, // Scale 2.0 for high-DPI rendering; PreviewUI handles area fit and zoom
                                );

                                if rx.has_changed().unwrap_or(false) {
                                    return;
                                }

                                if let Some(rendered_img) = img {
                                    if let Ok(mut guard) = image_state.lock() {
                                        *guard = Some(rendered_img);
                                        image_id.fetch_add(1, Ordering::Release);
                                        changed.store(true, Ordering::Release);
                                    }
                                    if let Ok(mut str_guard) = string_state.lock() {
                                        *str_guard = None;
                                    }
                                    if let Some(ref tx) = event_tx {
                                        let _ = tx.send(crate::message::Event::PreviewChange);
                                    }
                                    return;
                                }
                                // Fall through to text rendering if image failed
                            }

                            // Text / fallback rendering path
                            if is_mermaid {
                                let opts = crate::utils::mermaid::MermaidOptions {
                                    max_width: width,
                                    show_box: true,
                                    title: Some("Mermaid Diagram".to_string()),
                                    ..Default::default()
                                };
                                let rendered_text =
                                    crate::utils::mermaid::render_mermaid_file(p, &opts)
                                        .unwrap_or_else(|err| {
                                            Text::from(format!("Error reading Mermaid file: {err}"))
                                        });

                                if rx.has_changed().unwrap_or(false) {
                                    return;
                                }
                                if let Ok(mut guard) = string_state.lock() {
                                    *guard = Some(rendered_text);
                                    changed.store(true, Ordering::Release);
                                }
                                if let Ok(mut img_guard) = image_state.lock() {
                                    *img_guard = None;
                                    image_id.fetch_add(1, Ordering::Release);
                                }
                                if let Some(ref tx) = event_tx {
                                    let _ = tx.send(crate::message::Event::PreviewChange);
                                }
                            } else {
                                // Markdown file — render with diagram offset tracking
                                let opts = crate::utils::markdown::MarkdownOptions {
                                    max_width: width,
                                    render_mermaid: markdown_diagrams,
                                    ..Default::default()
                                };
                                let (rendered_text, offsets) =
                                    crate::utils::markdown::render_markdown_file_with_diagram_offsets(
                                        p, &opts,
                                    )
                                    .unwrap_or_else(|err| {
                                        (
                                            Text::from(format!("Error reading Markdown file: {err}")),
                                            vec![],
                                        )
                                    });

                                if rx.has_changed().unwrap_or(false) {
                                    return;
                                }

                                // If media is enabled, extract and prepare diagram image
                                let mut diag_img = None;
                                let mut sources = Vec::new();
                                if media_enabled {
                                    if let Ok(content) = std::fs::read_to_string(p) {
                                        let diagrams =
                                            crate::utils::markdown::extract_mermaid_blocks(&content);
                                        sources =
                                            diagrams.into_iter().map(|(_, s)| s).collect();
                                        if let Some(first_diag) = sources.first() {
                                            diag_img =
                                                crate::utils::mermaid::render_mermaid_to_image(
                                                    first_diag, 2.0,
                                                );
                                        }
                                    }
                                }

                                if rx.has_changed().unwrap_or(false) {
                                    return;
                                }

                                if let Ok(mut img_guard) = image_state.lock() {
                                    *img_guard = diag_img;
                                    image_id.fetch_add(1, Ordering::Release);
                                }
                                if let Ok(mut src_guard) = diagram_sources_state.lock() {
                                    *src_guard = sources;
                                }
                                current_diagram_idx_state.store(
                                    0,
                                    std::sync::atomic::Ordering::Release,
                                );

                                // Store diagram offsets for navigation
                                if let Ok(mut diag_guard) = diagram_offsets_state.lock() {
                                    *diag_guard = offsets;
                                }
                                if let Ok(mut guard) = string_state.lock() {
                                    *guard = Some(rendered_text);
                                    changed.store(true, Ordering::Release);
                                }

                                if let Some(ref tx) = event_tx {
                                    let _ = tx.send(crate::message::Event::PreviewChange);
                                }
                            }
                        });
                        continue;
                    }
                    _ => {}
                }
            }

            if !matches!(m, PreviewMessage::Media(_) | PreviewMessage::Markdown(_, _)) {
                self.dispatch_kill();
                if !self.config.delay_clear {
                    self.clear_string();
                    self.clear_image();
                }
            }

            match m {
                PreviewMessage::Run(cmd, variables) => {
                    self.last = cmd.clone();
                    let mut cmd_builder = if let Some(s) = &self.config.shell
                        && s.len() > 0
                    {
                        let mut iter = s.into_iter();
                        let mut program = Command::new(iter.next().unwrap());
                        program.args(iter).arg(&cmd);
                        program
                    } else if let Some(direct) = try_parse_simple_command(&cmd) {
                        direct
                    } else {
                        Command::from_script(&cmd)
                    };

                    cmd_builder
                        .envs(variables.iter().cloned())
                        .stdout(Stdio::piped())
                        .stdin(Stdio::null())
                        .stderr(Stdio::null());

                    #[cfg(not(target_os = "windows"))]
                    cmd_builder.detach();

                    if !self.config.delay_clear {
                        self.lines.clear();
                        self.changed.store(true, Ordering::Relaxed);
                    }

                    // we need the child handle
                    if let Some(mut child) = cmd_builder._spawn() {
                        if let Some(stdout) = child.stdout.take() {
                            let lines = self.lines.clone();
                            let mut guard = self.lines.read();
                            let changed = self.changed.clone();
                            let cmd_str = cmd.clone();
                            let string_state = self.string.clone();
                            let image_state = self.image.clone();

                            // false => needs refresh (i.e. invalid utf-8)
                            let handle = tokio::spawn(async move {
                                let mut reader = BufReader::new(stdout);
                                let mut leftover = Vec::new();
                                let mut buf = [0u8; 8192];
                                let mut first = true;

                                while let Ok(n) = std::io::Read::read(&mut reader, &mut buf) {
                                    if n == 0 {
                                        break;
                                    }

                                    if first {
                                        if self.config.delay_clear {
                                            lines.clear();
                                            if let Ok(mut s) = string_state.lock() {
                                                *s = None;
                                            }
                                            if let Ok(mut img) = image_state.lock() {
                                                *img = None;
                                            }
                                            guard = lines.read(); // get new consistent snapshot
                                            changed.store(true, Ordering::Relaxed);
                                        }
                                        first = false;
                                    }

                                    leftover.extend_from_slice(&buf[..n]);

                                    let valid_up_to = match std::str::from_utf8(&leftover) {
                                        Ok(_) => leftover.len(),
                                        Err(e) => e.valid_up_to(),
                                    };

                                    let split_at = leftover[..valid_up_to]
                                        .iter()
                                        .rposition(|&b| b == b'\n' || b == b'\r')
                                        .map(|pos| pos + 1)
                                        .unwrap_or(valid_up_to);

                                    let (valid_bytes, rest) = leftover.split_at(split_at);

                                    match valid_bytes.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                // re-check before pushing
                                                if lines.is_expired(&guard) {
                                                    return true;
                                                }
                                                guard.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in valid_bytes.split(|b| *b == b'\n') {
                                                    if lines.is_expired(&guard) {
                                                        return true;
                                                    }
                                                    let line =
                                                        String::from_utf8_lossy(bytes).into_owned();
                                                    guard.push(Line::from(line));
                                                }
                                            } else {
                                                error!("Error displaying {cmd_str}: {:?}", e);
                                                return false;
                                            }
                                        }
                                    }

                                    leftover = rest.to_vec();
                                }

                                // no lines read, clear
                                if first && self.config.delay_clear {
                                    lines.clear();
                                    if let Ok(mut s) = string_state.lock() {
                                        *s = None;
                                    }
                                    if let Ok(mut img) = image_state.lock() {
                                        *img = None;
                                    }
                                    changed.store(true, Ordering::Relaxed);
                                } else if !leftover.is_empty() && !lines.is_expired(&guard) {
                                    match leftover.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                if lines.is_expired(&guard) {
                                                    return true;
                                                }
                                                guard.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in leftover.split(|b| *b == b'\n') {
                                                    if lines.is_expired(&guard) {
                                                        return true;
                                                    }
                                                    let line =
                                                        String::from_utf8_lossy(bytes).into_owned();
                                                    guard.push(Line::from(line));
                                                }
                                            } else {
                                                error!("Error displaying {cmd_str}: {:?}", e);
                                                return false;
                                            }
                                        }
                                    }
                                }

                                true
                            });
                            self.current = Some((child, handle))
                        } else {
                            error!("Failed to get stdout of preview command: {cmd}")
                        }
                    }
                }
                PreviewMessage::Stop => {
                    self.lines.clear();
                    self.changed.store(true, Ordering::Relaxed);
                    self.last.clear();
                }
                _ => {}
            }

            self.prune_procs();
        }

        let ret = self.cleanup_procs();
        if ret.is_empty() { Ok(()) } else { Err(ret) }
    }

    fn dispatch_kill(&mut self) {
        if let Some((mut child, old)) = self.current.take() {
            kill_child(&mut child);
            self.procs.push(child);

            // drop future
            let mut old = Box::pin(old);
            match old.as_mut().now_or_never() {
                Some(Ok(result)) => {
                    // unicode error
                    if !result {
                        self.send(Event::Refresh)
                    }
                }
                None => {
                    old.abort(); // still works because `AbortHandle` is separate
                }
                _ => {}
            }
        }
    }

    fn send(&self, event: Event) {
        if let Some(ref tx) = self.event_controller_tx {
            let _ = tx.send(event);
        }
    }

    pub fn connect_controller(&mut self, event_controller_tx: EventSender) {
        self.event_controller_tx = Some(event_controller_tx)
    }

    // todo: This would be cleaner with tokio::Child, but does that merit a conversion? I'm not sure if its worth it for the previewer to yield control while waiting for output cuz we are multithreaded anyways
    // also, maybe don't want this delaying exit?
    fn cleanup_procs(mut self) -> Vec<Child> {
        let total_timeout = Duration::from_secs(1);
        let start = Instant::now();

        self.procs.retain_mut(|child| {
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return false,
                    Ok(None) => {
                        if start.elapsed() >= total_timeout {
                            error!("Child failed to exit in time: {:?}", child);
                            return true;
                        } else {
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                    Err(e) => {
                        error!("Error waiting on child: {e}");
                        return true;
                    }
                }
            }
        });

        self.procs
    }

    fn prune_procs(&mut self) {
        self.procs.retain_mut(|child| match child.try_wait() {
            Ok(None) => {
                kill_child(child);
                true
            }
            Ok(Some(_)) => false,
            Err(e) => {
                warn!("Error waiting on child: {e}");
                true
            }
        });
    }
}

fn kill_child(child: &mut Child) {
    let pid = child.id();

    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let pgid = Pid::from_raw(-(pid as i32));
        let _ = kill(pgid, Signal::SIGKILL);
    }
    let _ = child.kill();
}

// ---------- NON ANSI VARIANT
// let reader = BufReader::new(stdout);
// if self.config.try_lossy {
// for line_result in reader.split(b'\n') {
//     match line_result {
//         Ok(bytes) => {
//             let line =
//             String::from_utf8_lossy(&bytes).into_owned();
//             lines.push(Line::from(line));
//         }
//         Err(e) => error!("Failed to read line: {:?}", e),
//     }
// }
// } else {
//     for line_result in reader.lines() {
//         match line_result {
//             Ok(line) => lines.push(Line::from(line)),
//             Err(e) => {
//                 // todo: don't know why that even with an explicit ratatui clear, garbage sometimes stays on the screen
//                 error!("Error displaying {cmd}: {:?}", e);
//                 break;
//             }
//         }
//     }
// }

// trait Resettable: Default {
//     fn reset(&mut self) {}
// }
// impl<T> Resettable for AppendOnly<T> {
//     fn reset(&mut self) {
//         self.clear();
//     }
// }

// use std::ops::{Deref, DerefMut};

// #[derive(Debug)]
// struct Queue<V: Resettable> {
//     entries: Vec<(String, V)>,
//     order: Vec<usize>, // indices ordered by recency (0 = most recent)
// }

// impl<V: Resettable> Queue<V> {
//     pub fn new(len: usize) -> Self {
//         Self {
//             entries: (0..len)
//             .map(|_| (String::default(), V::default()))
//             .collect(),
//             order: vec![len; len],
//         }
//     }

//     fn find_key_pos(&self, key: &str) -> Option<(usize, usize)> {
//         for (order_idx, &entries_idx) in self.order.iter().enumerate() {
//             if order_idx == self.entries.len() {
//                 return None
//             }
//             if self.entries[entries_idx].0 == key {
//                 return Some((order_idx, entries_idx));
//             }
//         }
//         None
//     }

//     /// Try to get a key; if found, move it to the top.
//     /// If not found, replace the oldest, clear its vec, set new key.
//     pub fn try_get(&mut self, key: &str) -> bool {
//         let n = self.entries.len();

//         if !key.is_empty() && let Some((order_idx, idx)) = self.find_key_pos(key) {
//             self.order.copy_within(0..order_idx, 1);
//             self.order[0] = idx;
//             true
//         } else {
//             let order_idx = (0..n)
//             .rfind(|&i| self.order[i] < n)
//             .map(|i| i + 1)
//             .unwrap_or(0);

//             let idx = if self.order[order_idx] < self.entries.len() {
//                 order_idx
//             } else {
//                 *self.order.last().unwrap()
//             };

//             // shift and insert at front
//             self.order.copy_within(0..order_idx, 1);
//             self.order[0] = idx;

//             // reset and assign new key
//             let (ref mut k, ref mut v) = self.entries[idx];
//             *k = key.to_owned();
//             v.reset();

//             false
//         }
//     }
// }

// impl<V: Resettable> Deref for Queue<V> {
//     type Target = V;
//     fn deref(&self) -> &Self::Target {
//         &self.entries[self.order[0]].1
//     }
// }

// impl<V: Resettable> DerefMut for Queue<V> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.entries[self.order[0]].1
//     }
// }

// impl<V: Resettable> Default for Queue<V> {
//     fn default() -> Self {
//         Self::new(1)
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_parse_simple_command() {
        assert!(try_parse_simple_command("bat --color=always file.rs").is_some());
        assert!(try_parse_simple_command("cat /tmp/test.txt").is_some());
        assert!(try_parse_simple_command("ls -la src/").is_some());
        assert!(try_parse_simple_command("   ").is_none());

        // Metacharacters should fall back to shell (return None)
        assert!(try_parse_simple_command("bat file.rs | grep fn").is_none());
        assert!(try_parse_simple_command("echo hello > out.txt").is_none());
        assert!(try_parse_simple_command("cat file && echo ok").is_none());
        assert!(try_parse_simple_command("echo $VAR").is_none());
        assert!(try_parse_simple_command("cat `which bat`").is_none());
        assert!(try_parse_simple_command("echo 'hello world'").is_none());
        assert!(try_parse_simple_command("echo \"hello world\"").is_none());
    }
}
