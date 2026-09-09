use std::{path::PathBuf, process::Command, str::FromStr};

use cba::{
    StringError, bait::ResultExt, bring::split::split_on_unescaped_delimiter, broc::CommandExt,
    unwrap,
};
use log::{debug, error};
use matchmaker::{
    Action, Actions, ConfigMMInnerItem, ConfigMMItem,
    binds::Trigger,
    config::PartialRenderConfig,
    event::BindSender,
    message::{BindDirective, Interrupt, RenderCommand},
    nucleo::Line,
    ui::StatusUI,
};
use matchmaker_partial::{Apply, Set};

use matchmaker::preview::AppendOnly;

pub type MMState<'a, 'b> = matchmaker::render::MMState<'a, 'b, ConfigMMItem, ConfigMMInnerItem>;

#[derive(Debug, Clone, PartialEq)]
pub enum MMAction {
    // binds
    /// define a bind
    Bind(String),
    /// unset a bind
    Unbind(String),
    /// append actions to a bind
    PushBind(String),
    /// pop an action from a bind
    PopBind(String),

    // state
    /// Toggle refiltering of results by query.
    Filtering(Option<bool>),
    /// Cycle result sorting between None, Partial, and Full
    CycleSort,
    ReloadNext(Option<usize>),
    ReloadPrev,

    // set
    /// Set header
    SetHeader(Option<String>),
    /// Push header
    PushHeader(String),
    /// Set footer
    SetFooter(Option<String>),
    /// Push footer
    PushFooter(String),
    /// Set status without interpreting style braces
    SetPrompt(Option<String>),
    /// Set prompt
    SetStyledPrompt(String),
    /// Set status without interpreting style braces
    SetStatus(Option<String>),
    /// Set status
    SetStyledStatus(String),
    /// Set current mode index (0 = local, 1 = frecency, 2 = bookmarks)
    SetModeIndex(usize),
    /// Run a command and display output in preview window (TODO)
    RunPreview(String),

    /// Accept current selection and print using output_template
    Accept,

    // Unimplemented
    /// History up (TODO)
    HistoryUp,
    /// History down (TODO)
    HistoryDown,
    /// [`matchmaker::Action::Execute`], confirm on error
    ExecuteOrConfirm(String),
    /// [`matchmaker::Action::Execute`], quit on success
    ExecuteAndQuit(String),
    /// [`matchmaker::Action::Execute`], quit on success, confirm on error
    BecomeOr(String),
    /// Execute command and parse output as actions
    Transform(String),
    /// Execute command and parse output as configuration
    TransformConfig(String),

    /// Set the set of col-0 paths shown with the yank prefix style (FM mode).
    /// Value is a newline-separated list of paths (empty string clears).
    FmSetYankPaths(String),
    FmRemoveYankPaths(String),
    FmSetCutPaths(String),
    FmRemoveCutPaths(String),
    FmSetPinPaths(String),

    /// File-manager action-box operations.
    FmCreateStart,
    FmDeleteStart,
    FmRenameStart,
    FmUnzipStart,
    FmZipStart,
    FmYank,
    FmUnyank,
    FmCut,
    FmUncut,
    FmPaste,
    FmPasteInto,
    FmSetFlashCopy(String),
    FmSetFlashCut(String),
    FmUndo,
    FmRedo,
    FmDragDrop,
    FmTogglePin,
    ReloadReady(Vec<String>),
    Confirm(String),
    Prompt(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FmActionMode {
    Create,
    Delete {
        paths: Vec<String>,
    },
    Rename {
        from: String,
        remaining: Vec<String>,
    },
    Unzip {
        src: String,
    },
    Zip {
        paths: Vec<String>,
    },
    Custom {
        command: String,
    },
}

#[derive(Debug, Clone)]
pub struct ModeHistory {
    pub query: String,
    pub query_cursor: usize,
    pub results_index: u32,
    pub focused_item: Option<String>,
    pub focus: matchmaker::render::Focus,
}

pub struct ActionContext {
    pub bind_tx: BindSender<MMAction>,
    pub render_tx: matchmaker::event::RenderSender<MMAction>,
    pub additional_commands: (Vec<String>, usize),
    pub output_template: Option<String>,
    pub print_handle: AppendOnly<String>,
    pub output_separator: String,
    pub clipboard: crate::fm::Clipboard,
    pub fm_notify: bool,
    pub undo_stack: crate::fm::UndoStack,
    pub redo_stack: crate::fm::UndoStack,
    pub fm_action: Option<FmActionMode>,
    pub mode_history: std::collections::HashMap<usize, ModeHistory>,
    pub last_cwd: Option<std::path::PathBuf>,
}

pub fn action_handler(
    a: MMAction,
    state: &mut MMState<'_, '_>,
    ActionContext {
        bind_tx,
        render_tx,
        additional_commands,
        output_template,
        print_handle,
        output_separator,
        clipboard,
        fm_notify,
        undo_stack,
        redo_stack,
        fm_action,
        mode_history,
        last_cwd,
    }: &mut ActionContext,
) {
    if let Ok(cwd) = std::env::current_dir() {
        if last_cwd.as_ref() != Some(&cwd) {
            *last_cwd = Some(cwd);
            mode_history.clear();
        }
    }
    match a {
        MMAction::Accept => {
            if state.picker_ui.action_visible {
                commit_fm_action(
                    state, render_tx, undo_stack, redo_stack, fm_action, *fm_notify,
                );
                return;
            }

            let repeat = |s: String| {
                use std::io::IsTerminal;
                if std::io::stdout().is_terminal() {
                    print_handle.push(s);
                } else {
                    print!("{}{}", s, output_separator);
                }
            };

            let cwd = std::env::current_dir().ok();
            let selected_items = state.map_selected_to_vec(|_, x| {
                let s = x.to_cow().to_string();
                if let Some(ref dir) = cwd {
                    let p = std::path::Path::new(&s);
                    if p.is_relative() {
                        let full = dir.join(p);
                        if full.exists() {
                            return full.to_string_lossy().to_string();
                        }
                    }
                }
                s
            });
            if state.picker_ui.worker.frecency {
                let store = matchmaker::frecency::FrecencyStore::open();
                for item in &selected_items {
                    let _ = store.add(item);
                }
            }

            if let Some(template) = output_template {
                crate::formatter::format_cli(state, template, Some(&repeat));
            } else {
                for item in selected_items {
                    repeat(item);
                }
            }

            state.should_quit = true;
        }
        // state
        MMAction::CycleSort => {
            #[cfg(feature = "experimental")]
            {
                let threshold = match state.picker_ui.worker.get_stability() {
                    0 => 6,
                    u32::MAX => 0,
                    _ => u32::MAX,
                };
                state
                    .picker_ui
                    .worker
                    .set_stability(matchmaker::config::SortThreshold(threshold));
            }
        }
        MMAction::Filtering(s) => {
            if let Some(s) = s {
                state.filtering = s
            } else {
                state.filtering = !state.filtering
            }
        }

        // history
        MMAction::HistoryUp => {
            // todo
        }
        MMAction::HistoryDown => {
            // todo
        }

        MMAction::ReloadNext(x) => {
            if additional_commands.0.is_empty() {
                return;
            }

            let old_index = additional_commands.1;
            let index = match x {
                None => {
                    additional_commands.1 =
                        (additional_commands.1 + 1) % additional_commands.0.len();
                    additional_commands.1
                }
                Some(x) => {
                    if x < additional_commands.0.len() {
                        let target = if additional_commands.1 == x && x != 0 {
                            0
                        } else {
                            x
                        };
                        additional_commands.1 = target;
                        target
                    } else {
                        error!("Index {x} is out of bounds for ReloadNext");
                        return;
                    }
                }
            };

            if old_index != index {
                let focused_item = state.current_raw().and_then(|item| {
                    state
                        .picker_ui
                        .worker
                        .columns
                        .first()
                        .map(|c| c.raw(item).into_owned())
                });
                mode_history.insert(
                    old_index,
                    ModeHistory {
                        query: state.picker_ui.query.input.clone(),
                        query_cursor: state.picker_ui.query.cursor,
                        results_index: state.picker_ui.results.index(),
                        focused_item,
                        focus: state.focus,
                    },
                );
                if let Some(saved) = mode_history.get(&index) {
                    state
                        .picker_ui
                        .query
                        .set(Some(saved.query.clone()), saved.query_cursor as u16);
                    if let Some(ref target) = saved.focused_item {
                        *crate::start::TARGET_ITEM.lock().unwrap() = Some(target.clone());
                        unsafe {
                            std::env::set_var("MM_TARGET_ITEM", target);
                        }
                    }
                } else {
                    state.picker_ui.query.set(Some(String::new()), 0);
                    *crate::start::TARGET_ITEM.lock().unwrap() = None;
                    unsafe {
                        std::env::remove_var("MM_TARGET_ITEM");
                    }
                }
            }

            let payload = &additional_commands.0[index];
            state.envs.set("MM_INDEX", index);
            state.picker_ui.query.set_mode_index(index);
            state.picker_ui.results.set_mode_index(index);
            state.picker_ui.worker.set_mode_index(index);
            state.picker_ui.results.set_status_line(None);
            state.set_interrupt(Interrupt::Reload, payload.clone());

            let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                MMAction::SetModeIndex(index),
            )));

            if state.ui.config.nav_mode {
                let focus_to_set = if index != 0 {
                    matchmaker::render::Focus::Input
                } else if let Some(saved) = mode_history.get(&index) {
                    saved.focus
                } else {
                    matchmaker::render::Focus::Results
                };
                state.focus = focus_to_set;
                match focus_to_set {
                    matchmaker::render::Focus::Results => {
                        let _ = render_tx.send(RenderCommand::Action(Action::FocusNav));
                    }
                    matchmaker::render::Focus::Input => {
                        let _ = render_tx.send(RenderCommand::Action(Action::FocusFilter));
                    }
                }
            } else {
                state.focus = matchmaker::render::Focus::Input;
                let _ = render_tx.send(RenderCommand::Action(Action::FocusFilter));
            }

            let has_target = mode_history
                .get(&index)
                .and_then(|s| s.focused_item.as_ref())
                .is_some()
                || crate::start::TARGET_ITEM.lock().unwrap().is_some();
            if !has_target {
                if let Some(pos) = mode_history.get(&index).map(|s| s.results_index) {
                    if pos > 0 {
                        let _ = render_tx.send(RenderCommand::Action(Action::Pos(pos as i32)));
                        state.picker_ui.results.cursor_jump(pos);
                    }
                }
            }
        }

        MMAction::ReloadPrev => {
            if additional_commands.0.is_empty() {
                return;
            }

            let old_index = additional_commands.1;
            additional_commands.1 = (additional_commands.1 + additional_commands.0.len() - 1)
                % additional_commands.0.len();

            let index = additional_commands.1;

            if old_index != index {
                let focused_item = state.current_raw().and_then(|item| {
                    state
                        .picker_ui
                        .worker
                        .columns
                        .first()
                        .map(|c| c.raw(item).into_owned())
                });
                mode_history.insert(
                    old_index,
                    ModeHistory {
                        query: state.picker_ui.query.input.clone(),
                        query_cursor: state.picker_ui.query.cursor,
                        results_index: state.picker_ui.results.index(),
                        focused_item,
                        focus: state.focus,
                    },
                );
                if let Some(saved) = mode_history.get(&index) {
                    state
                        .picker_ui
                        .query
                        .set(Some(saved.query.clone()), saved.query_cursor as u16);
                    if let Some(ref target) = saved.focused_item {
                        *crate::start::TARGET_ITEM.lock().unwrap() = Some(target.clone());
                        unsafe {
                            std::env::set_var("MM_TARGET_ITEM", target);
                        }
                    }
                } else {
                    state.picker_ui.query.set(Some(String::new()), 0);
                    *crate::start::TARGET_ITEM.lock().unwrap() = None;
                    unsafe {
                        std::env::remove_var("MM_TARGET_ITEM");
                    }
                }
            }

            let payload = &additional_commands.0[index];

            state.envs.set("MM_INDEX", index);
            state.picker_ui.query.set_mode_index(index);
            state.picker_ui.results.set_mode_index(index);
            state.picker_ui.worker.set_mode_index(index);
            state.picker_ui.results.set_status_line(None);

            state.set_interrupt(Interrupt::Reload, payload.clone());

            let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                MMAction::SetModeIndex(index),
            )));

            if state.ui.config.nav_mode {
                let focus_to_set = if index != 0 {
                    matchmaker::render::Focus::Input
                } else if let Some(saved) = mode_history.get(&index) {
                    saved.focus
                } else {
                    matchmaker::render::Focus::Results
                };
                state.focus = focus_to_set;
                match focus_to_set {
                    matchmaker::render::Focus::Results => {
                        let _ = render_tx.send(RenderCommand::Action(Action::FocusNav));
                    }
                    matchmaker::render::Focus::Input => {
                        let _ = render_tx.send(RenderCommand::Action(Action::FocusFilter));
                    }
                }
            } else {
                state.focus = matchmaker::render::Focus::Input;
                let _ = render_tx.send(RenderCommand::Action(Action::FocusFilter));
            }

            let has_target = mode_history
                .get(&index)
                .and_then(|s| s.focused_item.as_ref())
                .is_some()
                || crate::start::TARGET_ITEM.lock().unwrap().is_some();
            if !has_target {
                if let Some(pos) = mode_history.get(&index).map(|s| s.results_index) {
                    if pos > 0 {
                        let _ = render_tx.send(RenderCommand::Action(Action::Pos(pos as i32)));
                        state.picker_ui.results.cursor_jump(pos);
                    }
                }
            }
        }

        MMAction::ReloadReady(_) => {
            state.reloading = false;
            state.picker_ui.worker.nucleo.tick(20);
            state.picker_ui.update();

            let target_opt = crate::start::TARGET_ITEM
                .lock()
                .unwrap()
                .clone()
                .or_else(|| std::env::var("MM_TARGET_ITEM").ok());

            let mut target_found = false;

            if let Some(ref target) = target_opt {
                let count = state.picker_ui.worker.counts().0;
                let target_trimmed = target.trim_end_matches('/');
                for i in 0..count {
                    if let Some(raw) = state.picker_ui.worker.get_nth(i) {
                        let val = state.picker_ui.worker.columns[0].raw(raw);
                        let val_trimmed = val.trim_end_matches('/');
                        let val_is_abs =
                            val_trimmed.starts_with('/') || val_trimmed.starts_with('\\');
                        let target_is_abs =
                            target_trimmed.starts_with('/') || target_trimmed.starts_with('\\');
                        let is_match = if val_trimmed == target_trimmed {
                            true
                        } else if val_trimmed.trim_start_matches("./")
                            == target_trimmed.trim_start_matches("./")
                        {
                            true
                        } else if val_is_abs && target_is_abs {
                            false
                        } else if !val_is_abs && target_is_abs {
                            target_trimmed.ends_with(&format!("/{}", val_trimmed))
                        } else if val_is_abs && !target_is_abs {
                            state.picker_ui.worker.mode_index == 0
                                && val_trimmed.ends_with(&format!("/{}", target_trimmed))
                        } else {
                            val_trimmed.ends_with(&format!("/{}", target_trimmed))
                                || target_trimmed.ends_with(&format!("/{}", val_trimmed))
                        };
                        if is_match {
                            state.picker_ui.results.cursor_jump(i);
                            let _ = render_tx.send(RenderCommand::Action(Action::Pos(i as i32)));
                            target_found = true;
                            crate::start::TARGET_ITEM.lock().unwrap().take();
                            unsafe {
                                std::env::remove_var("MM_TARGET_ITEM");
                            }
                            break;
                        }
                    }
                }
            }

            if !target_found && target_opt.is_none() {
                state.picker_ui.results.cursor_jump(0);
                let _ = render_tx.send(RenderCommand::Action(Action::Pos(0)));
            }

            state.needs_redraw = true;
        }

        MMAction::RunPreview(cmd) => {
            if let Some(p) = state.preview_ui {
                p.show(true);
                state.update_preview_set(Ok(cmd));
            }
        }

        // binds
        MMAction::Bind(s) => {
            let (trigger, values) = unwrap!(parse_bind_parts(&s)._elog());
            let _ = bind_tx.send(BindDirective::Bind(trigger, values));
        }
        MMAction::Unbind(s) => {
            let trigger = unwrap!(s.parse()._elog());
            let _ = bind_tx.send(BindDirective::Unbind(trigger));
        }
        MMAction::PushBind(s) => {
            let (trigger, action) = unwrap!(parse_push_bind_parts(&s)._elog());
            let _ = bind_tx.send(BindDirective::PushBind(trigger, action));
        }
        MMAction::PopBind(s) => {
            let trigger = unwrap!(s.parse()._elog());
            let _ = bind_tx.send(BindDirective::PopBind(trigger));
        }

        // set
        MMAction::SetHeader(context) => {
            if let Some(s) = context {
                state.picker_ui.header.set(s);
            } else {
                state.picker_ui.header.clear(true);
            }
        }
        MMAction::PushHeader(s) => {
            state.picker_ui.header.push(s);
        }
        MMAction::SetFooter(context) => {
            if let Some(s) = context {
                state.footer_ui.set(s);
            } else {
                state.footer_ui.clear(false);
            }
        }
        MMAction::PushFooter(s) => {
            state.footer_ui.push(s);
        }
        MMAction::SetStyledPrompt(s) => {
            state
                .picker_ui
                .query
                .set_prompt(Some(StatusUI::parse_template_to_status_line(&s)));
        }
        MMAction::SetStyledStatus(s) => {
            state
                .picker_ui
                .results
                .set_status_line(Some(StatusUI::parse_template_to_status_line(&s)));
        }
        MMAction::SetModeIndex(index) => {
            state.picker_ui.query.set_mode_index(index);
            state.picker_ui.results.set_mode_index(index);
            state.picker_ui.worker.set_mode_index(index);
            state.picker_ui.results.set_status_line(None);
        }
        MMAction::SetStatus(s) => {
            state.picker_ui.results.set_status_line(s.map(Line::raw));
        }
        MMAction::SetPrompt(s) => {
            state.picker_ui.query.set_prompt(s.map(Line::raw));
        }
        MMAction::ExecuteOrConfirm(s) => {
            state.discriminant_payload = Some(0);
            state.set_interrupt(Interrupt::Execute, s);
        }
        MMAction::ExecuteAndQuit(s) => {
            state.discriminant_payload = Some(1);
            state.set_interrupt(Interrupt::Execute, s);
        }
        MMAction::BecomeOr(s) => {
            state.discriminant_payload = Some(2);
            state.set_interrupt(Interrupt::Execute, s);
        }
        MMAction::Transform(payload) => {
            let cmd = format_cli(state, &payload, None);
            if cmd.is_empty() {
                error!("Failed to format transform command: {payload}");
                return;
            }
            let vars = state.make_env_vars();

            let render_tx = render_tx.clone();
            if let Some(contents) = Command::from_script(&cmd)
                .envs(vars)
                .read_to_string()
                ._elog()
            {
                debug!("Transform output:\n{}", contents);

                for line in contents.lines() {
                    match Action::<MMAction>::from_str(line) {
                        Ok(action) => {
                            let _ = render_tx.send(RenderCommand::Action(action));
                        }
                        Err(_) => {
                            error!("Failed to parse action from transform output: {}", line);
                        }
                    }
                }
            }
        }
        MMAction::TransformConfig(payload) => {
            let cmd = format_cli(state, &payload, None);
            if cmd.is_empty() {
                error!("Failed to format transform-config command: {payload}");
                return;
            }
            let vars = state.make_env_vars();

            if let Some(contents) = Command::from_script(&cmd)
                .envs(vars)
                .read_to_string()
                ._elog()
            {
                debug!("TransformConfig output:\n{}", contents);

                let words: Vec<String> = contents.lines().map(|s| s.to_string()).collect();
                match crate::parse::get_pairs(words) {
                    Ok(pairs) => {
                        let mut partial = PartialRenderConfig::default();
                        for (path, val) in pairs {
                            let mut parts = split_on_unescaped_delimiter(&val, "|||");
                            if let Err(e) = crate::parse::try_split_kv(&mut parts, false) {
                                error!("Failed to split KV for {}: {e}", path.join("."));
                                continue;
                            }

                            if let Err(e) = partial.set(path.as_slice(), &parts) {
                                error!("Failed to set partial for {}: {e}", path.join("."));
                            }
                        }

                        log::debug!("Parsed config update: {partial:?}");

                        // Apply the partial to UI components
                        state.ui.config.apply(partial.ui);
                        state.picker_ui.query.config.apply(partial.query);
                        state.picker_ui.results.config.apply(partial.results);
                        state.picker_ui.results.status_config.apply(partial.status);
                        state.footer_ui.config.apply(partial.footer);
                        state.picker_ui.header.config.apply(partial.header);

                        if let Some(preview_ui) = state.preview_ui.as_mut() {
                            preview_ui.config.apply(partial.preview);
                        }

                        let _ = render_tx.send(RenderCommand::Refresh);
                    }
                    Err(e) => {
                        error!("Failed to parse pairs from TransformConfig output: {e}");
                    }
                }
            }
        }
        MMAction::FmSetYankPaths(raw) => {
            state.picker_ui.results.yank_paths = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            state.needs_redraw = true;
        }
        MMAction::FmRemoveYankPaths(raw) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let to_remove: Vec<String> = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            state.picker_ui.results.yank_paths.retain(|item| {
                !to_remove.iter().any(|rem| {
                    let rem_trimmed = rem.trim_end_matches('/').trim_end_matches('\\');
                    let item_trimmed = item.trim_end_matches('/').trim_end_matches('\\');
                    if rem_trimmed == item_trimmed {
                        return true;
                    }
                    let rem_abs = if std::path::Path::new(rem_trimmed).is_absolute() {
                        rem_trimmed.to_string()
                    } else {
                        cwd.join(rem_trimmed).to_string_lossy().to_string()
                    };
                    let item_abs = if std::path::Path::new(item_trimmed).is_absolute() {
                        item_trimmed.to_string()
                    } else {
                        cwd.join(item_trimmed).to_string_lossy().to_string()
                    };
                    rem_abs == item_abs
                })
            });
            state.needs_redraw = true;
        }
        MMAction::FmRemoveCutPaths(raw) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let to_remove: Vec<String> = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            state.picker_ui.results.cut_paths.retain(|item| {
                !to_remove.iter().any(|rem| {
                    let rem_trimmed = rem.trim_end_matches('/').trim_end_matches('\\');
                    let item_trimmed = item.trim_end_matches('/').trim_end_matches('\\');
                    if rem_trimmed == item_trimmed {
                        return true;
                    }
                    let rem_abs = if std::path::Path::new(rem_trimmed).is_absolute() {
                        rem_trimmed.to_string()
                    } else {
                        cwd.join(rem_trimmed).to_string_lossy().to_string()
                    };
                    let item_abs = if std::path::Path::new(item_trimmed).is_absolute() {
                        item_trimmed.to_string()
                    } else {
                        cwd.join(item_trimmed).to_string_lossy().to_string()
                    };
                    rem_abs == item_abs
                })
            });
            state.needs_redraw = true;
        }
        MMAction::FmSetCutPaths(raw) => {
            state.picker_ui.results.cut_paths = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            state.needs_redraw = true;
        }
        MMAction::FmSetPinPaths(raw) => {
            state.picker_ui.results.pin_paths = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            state.needs_redraw = true;
        }
        MMAction::FmSetFlashCopy(raw) => {
            state
                .picker_ui
                .results
                .set_flash_target(raw, matchmaker::ui::results::FlashOp::Copy);
            state.needs_redraw = true;
        }
        MMAction::FmSetFlashCut(raw) => {
            state
                .picker_ui
                .results
                .set_flash_target(raw, matchmaker::ui::results::FlashOp::Cut);
            state.needs_redraw = true;
        }
        MMAction::FmCreateStart => {
            *fm_action = Some(FmActionMode::Create);
            show_action_box(state, " ", "");
        }
        MMAction::FmDeleteStart => {
            let paths = fm_current_items(state);
            if !paths.is_empty() {
                let label = if paths.len() > 1 {
                    format!(
                        "{{red:}} Delete {} selected items? (Enter/Esc)",
                        paths.len()
                    )
                } else {
                    format!("{{red:}} {}? (Enter/Esc)", paths[0])
                };
                *fm_action = Some(FmActionMode::Delete { paths });
                show_styled_action_box(state, &label, "");
            }
        }
        MMAction::FmRenameStart => {
            let mut items = fm_current_items(state);
            if !items.is_empty() {
                let from = items.remove(0);
                *fm_action = Some(FmActionMode::Rename {
                    from: from.clone(),
                    remaining: items,
                });
                show_action_box(state, "󰑕 ", from.trim_end_matches('/'));
            }
        }
        MMAction::FmUnzipStart => {
            if let Some(src) = fm_current_items(state).into_iter().next() {
                *fm_action = Some(FmActionMode::Unzip { src: src.clone() });
                show_action_box(state, "󰋺 ", crate::fm::archive_stem(&src));
            }
        }
        MMAction::FmZipStart => {
            let paths = fm_current_items(state);
            if !paths.is_empty() {
                let default_name = if paths.len() == 1 {
                    format!("{}.zip", crate::fm::archive_stem(&paths[0]))
                } else {
                    "archive.zip".to_string()
                };
                *fm_action = Some(FmActionMode::Zip { paths });
                show_action_box(state, "󰋪 ", &default_name);
            }
        }
        MMAction::Confirm(payload) => {
            let (prompt_tmpl, cmd_tmpl) = if let Some((p, c)) = payload.split_once('|') {
                (p.trim(), c.trim())
            } else if let Some((p, c)) = payload.split_once(":::") {
                (p.trim(), c.trim())
            } else {
                (payload.as_str(), "")
            };

            let formatted_prompt = format_cli(state, prompt_tmpl, None);
            let formatted_cmd = format_cli(state, cmd_tmpl, None);

            *fm_action = Some(FmActionMode::Custom {
                command: formatted_cmd,
            });
            show_styled_action_box(state, &formatted_prompt, "");
        }
        MMAction::Prompt(payload) => {
            let parts: Vec<&str> = payload.split('|').collect();
            let (prompt_tmpl, cmd_tmpl, initial_tmpl) = match parts.len() {
                0 => ("", "", ""),
                1 => (parts[0].trim(), "", ""),
                2 => (parts[0].trim(), parts[1].trim(), ""),
                _ => (parts[0].trim(), parts[1].trim(), parts[2].trim()),
            };

            let formatted_prompt = crate::formatter::format_cli(state, prompt_tmpl, None);
            let formatted_cmd = crate::formatter::format_cli(state, cmd_tmpl, None);
            let formatted_initial = if !initial_tmpl.is_empty() {
                crate::formatter::format_cli(state, initial_tmpl, None)
            } else {
                String::new()
            };

            *fm_action = Some(FmActionMode::Custom {
                command: formatted_cmd,
            });
            show_styled_action_box(state, &formatted_prompt, &formatted_initial);
        }
        MMAction::FmYank => {
            let items = fm_current_items(state);
            if !items.is_empty() {
                let cwd = std::env::current_dir().unwrap_or_default();
                let abs_items: Vec<PathBuf> = items
                    .iter()
                    .map(|item| {
                        let path = PathBuf::from(item);
                        if path.is_absolute() {
                            path
                        } else {
                            cwd.join(path)
                        }
                    })
                    .collect();
                if let Ok(mut cb) = clipboard.lock() {
                    *cb = Some(crate::fm::FmClipboard {
                        items: abs_items,
                        op: crate::fm::ClipOp::Copy,
                    });
                }
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetFlashCopy(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetYankPaths(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetCutPaths(String::new()),
                )));
                if *fm_notify {
                    let msg = fm_notify_msg("Copied", &items, "{yellow}");
                    show_styled_info_box(state, &msg);
                }
            }
        }
        MMAction::FmUnyank => {
            let items = fm_current_items(state);
            if !items.is_empty() {
                // If it's in the clipboard, do we remove it?
                // For simplicity, we just remove it from yank_paths visually.
                // Or we can remove it from the clipboard if the clipboard exactly matches?
                // The primary goal is visual unyanking.
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmRemoveYankPaths(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmRemoveCutPaths(items.join("\n")),
                )));
                if *fm_notify {
                    let msg = fm_notify_msg("Un-yanked", &items, "{yellow}");
                    show_styled_info_box(state, &msg);
                }
            }
        }
        MMAction::FmUncut => {
            let items = fm_current_items(state);
            if !items.is_empty() {
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmRemoveYankPaths(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmRemoveCutPaths(items.join("\n")),
                )));
                if *fm_notify {
                    let msg = fm_notify_msg("Un-cut", &items, "{red}");
                    show_styled_info_box(state, &msg);
                }
            }
        }
        MMAction::FmCut => {
            let items = fm_current_items(state);
            if !items.is_empty() {
                let cwd = std::env::current_dir().unwrap_or_default();
                let abs_items: Vec<PathBuf> = items
                    .iter()
                    .map(|item| {
                        let path = PathBuf::from(item);
                        if path.is_absolute() {
                            path
                        } else {
                            cwd.join(path)
                        }
                    })
                    .collect();
                if let Ok(mut cb) = clipboard.lock() {
                    *cb = Some(crate::fm::FmClipboard {
                        items: abs_items,
                        op: crate::fm::ClipOp::Cut,
                    });
                }
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetFlashCut(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetCutPaths(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetYankPaths(String::new()),
                )));
                if *fm_notify {
                    let msg = fm_notify_msg("Cut", &items, "{red}");
                    show_styled_info_box(state, &msg);
                }
            }
        }
        MMAction::FmPaste => {
            let clip = clipboard.lock().ok().and_then(|g| g.clone());
            if let Some(clip) = clip {
                let cwd = std::env::current_dir().unwrap_or_default();
                let mut had_error = false;
                let mut pasted_items = Vec::new();

                for src in &clip.items {
                    let result = match clip.op {
                        crate::fm::ClipOp::Copy => crate::fm::copy_into(src, &cwd),
                        crate::fm::ClipOp::Cut => crate::fm::move_into(src, &cwd),
                    };
                    match result {
                        Ok(dest) => {
                            pasted_items.push((src.clone(), dest));
                        }
                        Err(e) => {
                            error!("fm paste '{}': {e}", src.display());
                            had_error = true;
                        }
                    }
                }

                if !pasted_items.is_empty() {
                    if let Ok(mut u) = undo_stack.lock() {
                        u.push(crate::fm::UndoAction::Paste {
                            op: clip.op,
                            items: pasted_items.clone(),
                            saved_clipboard: Some(clip.clone()),
                            saved_yank_paths: state.picker_ui.results.yank_paths.clone(),
                            saved_cut_paths: state.picker_ui.results.cut_paths.clone(),
                            previous_dir: None,
                            target_dir: None,
                        });
                    }
                }

                if !had_error {
                    // Record paste destination directory in frecency store
                    let store = matchmaker::frecency::FrecencyStore::open();
                    let _ = store.add(&cwd.to_string_lossy());

                    if let Ok(mut cb) = clipboard.lock() {
                        *cb = None;
                    }
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetCutPaths(String::new()),
                    )));
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetYankPaths(String::new()),
                    )));
                    state.picker_ui.results.cut_paths.clear();
                    state.picker_ui.results.yank_paths.clear();
                }

                if *fm_notify {
                    let names: Vec<String> = clip
                        .items
                        .iter()
                        .filter_map(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .collect();
                    let (verb, icon) = match clip.op {
                        crate::fm::ClipOp::Copy => ("Pasted", "󰆏"),
                        crate::fm::ClipOp::Cut => ("Moved", "󰆐"),
                    };
                    let color = if had_error { "{red}" } else { "{cyan}" };
                    let count = clip.items.len();
                    let item_word = if count == 1 { "item" } else { "items" };
                    let msg = format!(
                        "{color}{icon} {verb} {count} {item_word} ({}) into current folder (u to undo){{reset}}",
                        names.join(", ")
                    );
                    show_styled_info_box(state, &msg);
                }
                let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
            }
        }
        MMAction::FmPasteInto => {
            let clip = clipboard.lock().ok().and_then(|g| g.clone());
            if let Some(clip) = clip {
                let cwd = std::env::current_dir().unwrap_or_default();
                let focused_path = fm_current_items(state).into_iter().next().or_else(|| {
                    state.current_raw().and_then(|item| {
                        state
                            .picker_ui
                            .worker
                            .columns
                            .first()
                            .map(|c| c.raw(item).into_owned())
                    })
                });

                let (dest_dir, is_subfolder, dest_display_name) =
                    if let Some(ref raw) = focused_path {
                        let p = std::path::Path::new(raw);
                        let abs = if p.is_absolute() {
                            p.to_path_buf()
                        } else {
                            cwd.join(p)
                        };
                        let clean = raw.trim().trim_end_matches('/').trim_end_matches('\\');
                        if abs.is_dir() && clean != "." && clean != ".." && abs != cwd {
                            (abs, true, raw.clone())
                        } else {
                            (cwd.clone(), false, String::new())
                        }
                    } else {
                        (cwd.clone(), false, String::new())
                    };

                let mut had_error = false;
                let mut pasted_items = Vec::new();

                for src in &clip.items {
                    let result = match clip.op {
                        crate::fm::ClipOp::Copy => crate::fm::copy_into(src, &dest_dir),
                        crate::fm::ClipOp::Cut => crate::fm::move_into(src, &dest_dir),
                    };
                    match result {
                        Ok(dest) => {
                            pasted_items.push((src.clone(), dest));
                        }
                        Err(e) => {
                            error!("fm paste into '{}': {e}", src.display());
                            had_error = true;
                        }
                    }
                }

                if !pasted_items.is_empty() {
                    if let Ok(mut u) = undo_stack.lock() {
                        u.push(crate::fm::UndoAction::Paste {
                            op: clip.op,
                            items: pasted_items.clone(),
                            saved_clipboard: Some(clip.clone()),
                            saved_yank_paths: state.picker_ui.results.yank_paths.clone(),
                            saved_cut_paths: state.picker_ui.results.cut_paths.clone(),
                            previous_dir: if is_subfolder {
                                Some(cwd.clone())
                            } else {
                                None
                            },
                            target_dir: if is_subfolder {
                                Some(dest_dir.clone())
                            } else {
                                None
                            },
                        });
                    }
                }

                if !had_error {
                    let store = matchmaker::frecency::FrecencyStore::open();
                    let _ = store.add(&dest_dir.to_string_lossy());

                    if let Ok(mut cb) = clipboard.lock() {
                        *cb = None;
                    }
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetCutPaths(String::new()),
                    )));
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetYankPaths(String::new()),
                    )));
                    state.picker_ui.results.cut_paths.clear();
                    state.picker_ui.results.yank_paths.clear();
                }

                if is_subfolder {
                    let action = match clip.op {
                        crate::fm::ClipOp::Copy => {
                            MMAction::FmSetFlashCopy(dest_display_name.clone())
                        }
                        crate::fm::ClipOp::Cut => {
                            MMAction::FmSetFlashCut(dest_display_name.clone())
                        }
                    };
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(action)));
                }

                let count = clip.items.len();
                let item_word = if count == 1 { "item" } else { "items" };
                let (verb, icon) = match clip.op {
                    crate::fm::ClipOp::Copy => ("Pasted", "󰆏"),
                    crate::fm::ClipOp::Cut => ("Moved", "󰆐"),
                };
                let color = if had_error { "{red}" } else { "{cyan}" };
                let msg = if is_subfolder {
                    let clean_dest = dest_display_name
                        .trim()
                        .trim_start_matches("./")
                        .trim_end_matches('/')
                        .trim_end_matches('\\');
                    let target_display = if std::path::Path::new(clean_dest).is_absolute() {
                        format!("{clean_dest}/")
                    } else {
                        format!("./{clean_dest}/")
                    };
                    format!(
                        "{color}{icon} {verb} {count} {item_word} into {target_display} (u to undo){{reset}}"
                    )
                } else {
                    format!(
                        "{color}{icon} {verb} {count} {item_word} into current folder (u to undo){{reset}}"
                    )
                };
                show_styled_info_box(state, &msg);

                if is_subfolder && !pasted_items.is_empty() {
                    if let Some((_, dest)) = pasted_items.first() {
                        if let Some(item_name) = dest.file_name() {
                            let name = item_name.to_string_lossy().to_string();
                            *crate::start::TARGET_ITEM.lock().unwrap() = Some(name.clone());
                            unsafe {
                                std::env::set_var("MM_TARGET_ITEM", name);
                            }
                        }
                    }
                    let _ = render_tx.send(RenderCommand::Action(Action::ChDir(
                        dest_dir.to_string_lossy().to_string(),
                    )));
                }
                let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
            }
        }
        MMAction::FmUndo => {
            let action = undo_stack.lock().ok().and_then(|mut s| s.pop());
            if let Some(action) = action {
                if let Err(e) = crate::fm::apply_undo(&action) {
                    error!("fm undo: {e}");
                }

                if let crate::fm::UndoAction::Paste {
                    ref saved_clipboard,
                    ref saved_yank_paths,
                    ref saved_cut_paths,
                    ref previous_dir,
                    ref target_dir,
                    ..
                } = action
                {
                    if let Ok(mut cb) = clipboard.lock() {
                        *cb = saved_clipboard.clone();
                    }
                    state.picker_ui.results.yank_paths = saved_yank_paths.clone();
                    state.picker_ui.results.cut_paths = saved_cut_paths.clone();
                    let yanks: Vec<String> = saved_yank_paths.iter().cloned().collect();
                    let cuts: Vec<String> = saved_cut_paths.iter().cloned().collect();
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetYankPaths(yanks.join("\n")),
                    )));
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetCutPaths(cuts.join("\n")),
                    )));
                    if let Some(prev) = previous_dir {
                        if let Some(target) = target_dir {
                            if let Some(target_name) = target.file_name() {
                                let name = target_name.to_string_lossy().to_string();
                                *crate::start::TARGET_ITEM.lock().unwrap() = Some(name.clone());
                                unsafe {
                                    std::env::set_var("MM_TARGET_ITEM", name);
                                }
                            }
                        }
                        let _ = render_tx.send(RenderCommand::Action(Action::ChDir(
                            prev.to_string_lossy().to_string(),
                        )));
                    }
                }

                let msg = match &action {
                    crate::fm::UndoAction::Paste { op, items, .. } => {
                        let count = items.len();
                        let item_word = if count == 1 { "item" } else { "items" };
                        let op_word = match op {
                            crate::fm::ClipOp::Copy => "copy",
                            crate::fm::ClipOp::Cut => "move",
                        };
                        format!(
                            "{{yellow}}󰕌 Undone {op_word} of {count} {item_word} (clipboard restored){{reset}}"
                        )
                    }
                    crate::fm::UndoAction::DeletedFile { original, .. } => {
                        format!("{{yellow}}󰕌 Restored: {}{{reset}}", original.display())
                    }
                    crate::fm::UndoAction::DeletedFiles { items } => {
                        let count = items.len();
                        let item_word = if count == 1 { "item" } else { "items" };
                        format!("{{yellow}}󰕌 Restored {count} {item_word}{{reset}}")
                    }
                    crate::fm::UndoAction::CreatedFile { path } => {
                        format!(
                            "{{yellow}}󰕌 Undone creation of: {}{{reset}}",
                            path.display()
                        )
                    }
                    crate::fm::UndoAction::Renamed { from, to } => {
                        format!(
                            "{{yellow}}󰕌 Reverted rename: {} -> {}{{reset}}",
                            to.display(),
                            from.display()
                        )
                    }
                    crate::fm::UndoAction::Copied { dest } => {
                        format!("{{yellow}}󰕌 Removed copy: {}{{reset}}", dest.display())
                    }
                    crate::fm::UndoAction::Moved { from, to } => {
                        format!(
                            "{{yellow}}󰕌 Moved back: {} -> {}{{reset}}",
                            to.display(),
                            from.display()
                        )
                    }
                };

                if let Ok(mut rs) = redo_stack.lock() {
                    rs.push(action);
                }

                show_styled_info_box(state, &msg);
                let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
            }
        }
        MMAction::FmRedo => {
            let action = redo_stack.lock().ok().and_then(|mut s| s.pop());
            if let Some(action) = action {
                if let Err(e) = crate::fm::apply_redo(&action) {
                    error!("fm redo: {e}");
                }

                if let crate::fm::UndoAction::Paste {
                    ref items,
                    ref target_dir,
                    ..
                } = action
                {
                    if let Ok(mut cb) = clipboard.lock() {
                        *cb = None;
                    }
                    state.picker_ui.results.yank_paths.clear();
                    state.picker_ui.results.cut_paths.clear();
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetYankPaths(String::new()),
                    )));
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::FmSetCutPaths(String::new()),
                    )));
                    if let Some(target) = target_dir {
                        if let Some((_, dest)) = items.first() {
                            if let Some(item_name) = dest.file_name() {
                                let name = item_name.to_string_lossy().to_string();
                                *crate::start::TARGET_ITEM.lock().unwrap() = Some(name.clone());
                                unsafe {
                                    std::env::set_var("MM_TARGET_ITEM", name);
                                }
                            }
                        }
                        let _ = render_tx.send(RenderCommand::Action(Action::ChDir(
                            target.to_string_lossy().to_string(),
                        )));
                    }
                }

                let msg = match &action {
                    crate::fm::UndoAction::Paste { op, items, .. } => {
                        let count = items.len();
                        let item_word = if count == 1 { "item" } else { "items" };
                        let op_word = match op {
                            crate::fm::ClipOp::Copy => "pasted",
                            crate::fm::ClipOp::Cut => "moved",
                        };
                        format!("{{cyan}}󰑖 Redone {op_word} of {count} {item_word}{{reset}}")
                    }
                    _ => "{cyan}󰑖 Redone last action{reset}".to_string(),
                };

                if let Ok(mut us) = undo_stack.lock() {
                    us.push(action);
                }

                show_styled_info_box(state, &msg);
                let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
            }
        }
        MMAction::FmDragDrop => {
            let paths = fm_current_items(state);
            if !paths.is_empty() {
                let cwd = std::env::current_dir().unwrap_or_default();
                let mut cmd = std::process::Command::new("ripdrag");
                cmd.args(&paths);
                cmd.current_dir(cwd);
                if let Err(e) = cmd.spawn() {
                    log::error!("fm dragdrop: {e}");
                }
            }
        }
        MMAction::FmTogglePin => {
            let mut paths = fm_current_items(state);
            if paths.is_empty() {
                if let Ok(cwd) = std::env::current_dir() {
                    paths.push(cwd.to_string_lossy().to_string());
                }
            }
            if !paths.is_empty() {
                let store = matchmaker::frecency::FrecencyStore::open();
                let mut last_state = false;
                for p in &paths {
                    if let Ok(pinned) = store.toggle_pin(p) {
                        last_state = pinned;
                        let key_str = matchmaker::frecency::normalize_path(p);
                        if pinned {
                            state.picker_ui.results.pin_paths.insert(key_str);
                        } else {
                            state.picker_ui.results.pin_paths.remove(&key_str);
                            state.picker_ui.results.pin_paths.remove(p);
                        }
                    }
                }
                let verb = if last_state {
                    "Bookmarked"
                } else {
                    "Unbookmarked"
                };
                let is_dir = paths
                    .first()
                    .map(|p| std::path::Path::new(p).is_dir())
                    .unwrap_or(false);
                let icon_str = if is_dir {
                    "{yellow:󰮟}"
                } else {
                    "{yellow:󱀻}"
                };
                let color = if last_state { icon_str } else { "{darkgray}" };
                let msg = fm_notify_msg(verb, &paths, color);
                show_styled_info_box(state, &msg);
                let _ = render_tx.send(RenderCommand::Action(Action::Redraw));
                let _ = render_tx.send(RenderCommand::Refresh);
            }
        }
    }
}

impl MMAction {
    /// Validate Bind/PushBind/Unbind/PopBind instructions
    pub fn validate(&self) -> Result<(), StringError> {
        match self {
            MMAction::Bind(s) => {
                let (_trigger, actions) = crate::action::parse_bind_parts(s)?;
                for a in &actions {
                    if let Action::Custom(mm) = a {
                        mm.validate()?;
                    }
                }
            }
            MMAction::PushBind(s) => {
                let (_trigger, a) = crate::action::parse_push_bind_parts(s)?;
                if let Action::Custom(mm) = &a {
                    mm.validate()?;
                }
            }
            MMAction::Unbind(s) | MMAction::PopBind(s) => {
                s.parse::<Trigger>()?;
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn parse_bind_parts(s: &str) -> Result<(Trigger, Actions<MMAction>), StringError> {
    let (trigger, values) = s
        .split_once('=')
        .ok_or_else(|| format!("Expected '=' in Bind({s})"))?;

    let trigger = trigger.trim().parse()?;

    let parts = split_on_unescaped_delimiter(values, "|||");

    let actions = parts
        .iter()
        .map(|p| Action::<MMAction>::from_str(p.trim()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok((trigger, Actions::from_iter(actions)))
}

pub fn parse_push_bind_parts(s: &str) -> Result<(Trigger, Action<MMAction>), StringError> {
    let s = s.trim();
    let (trigger, values) = s
        .split_once('=')
        .ok_or_else(|| format!("Expected '=' in PushBind({s})"))?;

    let trigger = trigger.trim().parse()?;
    let action = Action::<MMAction>::from_str(values.trim())?;

    Ok((trigger, action))
}

enum_from_str_display! {
    MMAction;

    units:
    CycleSort, HistoryUp, HistoryDown, Accept, ReloadPrev, FmCreateStart, FmDeleteStart, FmRenameStart, FmUnzipStart, FmZipStart, FmYank, FmUnyank, FmCut, FmUncut, FmPaste, FmPasteInto, FmUndo, FmRedo, FmDragDrop, FmTogglePin;


    tuples:
    Bind, Unbind, PushBind, PopBind, ExecuteOrConfirm, ExecuteAndQuit, BecomeOr, Transform, TransformConfig, SetStyledPrompt, SetStyledStatus, SetModeIndex, PushHeader, PushFooter, RunPreview, FmSetYankPaths, FmRemoveYankPaths, FmSetCutPaths, FmRemoveCutPaths, FmSetPinPaths, FmSetFlashCopy, FmSetFlashCut, Confirm, Prompt;

    defaults:
    ;

    options:
    SetPrompt, SetHeader, SetFooter, SetStatus, Filtering, ReloadNext;

    lossy:
    ;
}

//------------------------------------------------
macro_rules! enum_from_str_display {
    (
        $enum:ty;
        units: $( $unit:ident ),* $(,)?;
        tuples: $( $tuple:ident ),* $(,)?;
        defaults: $(($default:ident, $default_value:expr)),*;
        options: $($optional:ident),*;
        lossy: $( $lossy:ident ),* ;
    ) => {
        impl std::fmt::Display for $enum {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use $enum::*;
                match self {
                    $( $unit => write!(f, stringify!($unit)), )*

                    $( $tuple(inner) => write!(f, concat!(stringify!($tuple), "({})"), inner), )*

                    $( $default(inner) => {
                        if *inner == $default_value {
                            write!(f, stringify!($default))
                        } else {
                            write!(f, concat!(stringify!($default), "({})"), inner)
                        }
                    }, )*

                    $( $optional(opt) => {
                        if let Some(inner) = opt {
                            write!(f, concat!(stringify!($optional), "({})"), inner)
                        } else {
                            write!(f, stringify!($optional))
                        }
                    }, )*

                    $( $lossy(inner) => {
                        if inner.is_empty() {
                            write!(f, stringify!($pathbuf))
                        } else {
                            write!(f, concat!(stringify!($lossy), "({})"), std::ffi::OsString::from(inner).to_string_lossy())
                        }
                    }, )*

                    /* ---------- Manually parsed ---------- */
                    Self::ReloadReady(_) => write!(f, "ReloadReady"),
                    /* ------------------------------------- */

                }
            }
        }

        impl std::str::FromStr for $enum {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let (name, data) = if let Some(pos) = s.find('(') {
                    if s.ends_with(')') {
                        (&s[..pos], Some(&s[pos + 1..s.len() - 1]))
                    } else {
                        (s, None)
                    }
                } else {
                    (s, None)
                };

                match name {
                    $( stringify!($unit) => {
                        if data.is_some() {
                            Err(format!("Unexpected data for {}", name))
                        } else {
                            Ok(Self::$unit)
                        }
                    }, )*

                    $( stringify!($tuple) => {
                        let val = data
                        .ok_or_else(|| format!("Missing data for {}", name))?
                        .parse()
                        .map_err(|_| format!("Invalid data for {}", name))?;
                        Ok(Self::$tuple(val))
                    }, )*

                    $( stringify!($lossy) => {
                        let d = match data {
                            Some(val) => val.parse()
                            .map_err(|_| format!("Invalid data for {}", stringify!($lossy)))?,
                            None => Default::default(),
                        };
                        Ok(Self::$lossy(d))
                    }, )*

                    $( stringify!($default) => {
                        let d = match data {
                            Some(val) => val.parse()
                            .map_err(|_| format!("Invalid data for {}", stringify!($default)))?,
                            None => $default_value,
                        };
                        Ok(Self::$default(d))
                    }, )*

                    $( stringify!($optional) => {
                        let d = match data {
                            Some(val) if !val.is_empty() => {
                                Some(val.parse().map_err(|_| format!("Invalid data for {}", stringify!($optional)))?)
                            }
                            _ => None,
                        };
                        Ok(Self::$optional(d))
                    }, )*

                    /* ---------- Manually parsed ---------- */
                    "ReloadReady" => Err("ReloadReady is internal only".into()),
                    /* ------------------------------------- */

                    _ => Err(format!("Unknown action {}", s)),
                }
            }
        }
    };
}
use enum_from_str_display;

use crate::formatter::format_cli;

fn show_action_box(state: &mut MMState<'_, '_>, prompt: &str, initial: &str) {
    state.picker_ui.action_visible = true;
    matchmaker::ACTION_BOX_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    state.picker_ui.action.set(Some(initial.to_string()), 0);
    state
        .picker_ui
        .action
        .set_prompt_line(StatusUI::parse_template_to_status_line(prompt));
}

fn show_styled_action_box(state: &mut MMState<'_, '_>, prompt: &str, initial: &str) {
    state.picker_ui.action_visible = true;
    matchmaker::ACTION_BOX_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    state.picker_ui.action.set(Some(initial.to_string()), 0);
    state
        .picker_ui
        .action
        .set_prompt_line(StatusUI::parse_template_to_status_line(prompt));
}

fn show_styled_info_box(state: &mut MMState<'_, '_>, message: &str) {
    state.picker_ui.action_visible = true;
    matchmaker::ACTION_BOX_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
    state.picker_ui.action.set(Some(String::new()), 0);
    state
        .picker_ui
        .action
        .set_prompt_line(StatusUI::parse_template_to_status_line(message));
    state.needs_redraw = true;
}

fn close_action_box(state: &mut MMState<'_, '_>, fm_action: &mut Option<FmActionMode>) {
    state.picker_ui.action_visible = false;
    matchmaker::ACTION_BOX_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
    state.picker_ui.action.set(Some(String::new()), 0);
    state.picker_ui.action.set_prompt(None);
    *fm_action = None;
}

fn commit_fm_action(
    state: &mut MMState<'_, '_>,
    render_tx: &matchmaker::event::RenderSender<MMAction>,
    undo_stack: &crate::fm::UndoStack,
    _redo_stack: &crate::fm::UndoStack,
    fm_action: &mut Option<FmActionMode>,
    fm_notify: bool,
) {
    let input = state.picker_ui.action.input.trim().to_string();
    let Some(mode) = fm_action.clone() else {
        close_action_box(state, fm_action);
        return;
    };

    match mode {
        FmActionMode::Create => {
            if !input.is_empty() {
                let result = if input.ends_with('/') {
                    std::fs::create_dir_all(&input)
                } else {
                    if let Some(parent) = std::path::Path::new(&input).parent()
                        && !parent.as_os_str().is_empty()
                    {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::File::create(&input).map(|_| ())
                };
                close_action_box(state, fm_action);
                if let Err(e) = result {
                    error!("fm create '{input}': {e}");
                } else {
                    if let Ok(mut stack) = undo_stack.lock() {
                        stack.push(crate::fm::UndoAction::CreatedFile {
                            path: PathBuf::from(&input),
                        });
                    }
                    if fm_notify {
                        let msg = format!("{{green:Created:}} {}", input);
                        show_styled_info_box(state, &msg);
                    }
                }
            } else {
                close_action_box(state, fm_action);
            }
        }
        FmActionMode::Delete { paths } => {
            let mut deleted_items = Vec::new();
            let mut deleted_names = Vec::new();
            for path in &paths {
                let path_buf = PathBuf::from(path);
                match crate::fm::move_to_trash(&path_buf) {
                    Ok(backup) => {
                        deleted_names.push(path.clone());
                        deleted_items.push((path_buf, backup));
                    }
                    Err(e) => error!("fm delete '{}': {e}", path),
                }
            }
            if !deleted_items.is_empty() {
                if let Ok(mut stack) = undo_stack.lock() {
                    stack.push(crate::fm::UndoAction::DeletedFiles {
                        items: deleted_items,
                    });
                }
            }
            close_action_box(state, fm_action);
            if fm_notify && !deleted_names.is_empty() {
                let msg = fm_notify_msg("Deleted", &deleted_names, "{red}");
                show_styled_info_box(state, &msg);
            }
        }
        FmActionMode::Rename { from, remaining } => {
            if !input.is_empty() && input != from {
                if let Err(e) =
                    crate::fm::move_path(std::path::Path::new(&from), std::path::Path::new(&input))
                {
                    error!("fm rename '{}' -> '{input}': {e}", from);
                } else if let Ok(mut stack) = undo_stack.lock() {
                    stack.push(crate::fm::UndoAction::Renamed {
                        from: PathBuf::from(&from),
                        to: PathBuf::from(&input),
                    });
                }
            }

            if !remaining.is_empty() {
                let mut remaining_mut = remaining.clone();
                let next_from = remaining_mut.remove(0);
                *fm_action = Some(FmActionMode::Rename {
                    from: next_from.clone(),
                    remaining: remaining_mut,
                });
                state
                    .picker_ui
                    .action
                    .set(Some(next_from.trim_end_matches('/').to_string()), 0);
                state.picker_ui.action_visible = true;
                matchmaker::ACTION_BOX_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
                return;
            }

            close_action_box(state, fm_action);
            if fm_notify && !input.is_empty() && input != from {
                let msg = format!("{{cyan:Renamed:}} {} -> {}", from, input);
                show_styled_info_box(state, &msg);
            }
        }
        FmActionMode::Unzip { src } => {
            if !input.is_empty() {
                let res = if let Err(e) = std::fs::create_dir_all(&input) {
                    Err(e)
                } else {
                    crate::fm::extract_archive(&src, &input)
                };
                close_action_box(state, fm_action);
                match res {
                    Ok(_) => {
                        if fm_notify {
                            let msg = format!("{{green:Extracted:}} {} -> {}", src, input);
                            show_styled_info_box(state, &msg);
                        }
                    }
                    Err(e) => error!("fm unzip '{src}' -> '{input}': {e}"),
                }
            } else {
                close_action_box(state, fm_action);
            }
        }
        FmActionMode::Zip { paths } => {
            if !input.is_empty() {
                let res = crate::fm::create_archive(&input, &paths);
                close_action_box(state, fm_action);
                match res {
                    Ok(_) => {
                        if fm_notify {
                            let msg = format!("{{green:Compressed:}} {}", input);
                            show_styled_info_box(state, &msg);
                        }
                    }
                    Err(e) => error!("fm zip '{input}': {e}"),
                }
            } else {
                close_action_box(state, fm_action);
            }
        }
        FmActionMode::Custom { command } => {
            if !command.is_empty() {
                let final_cmd = command.replace("{input}", &input);
                let vars = state.make_env_vars();
                if let Some(mut child) = Command::from_script(&final_cmd)
                    .envs(vars)
                    .stdin(crate::register::maybe_tty_in())
                    .stdout(crate::register::maybe_tty_out())
                    .stderr(crate::register::maybe_tty_out())
                    ._spawn()
                {
                    let _ = child.wait();
                }
            }
            close_action_box(state, fm_action);
        }
    }

    let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
}

fn fm_current_items(state: &MMState<'_, '_>) -> Vec<String> {
    state.map_selected_to_vec(|_, x| x.to_cow().to_string())
}

fn fm_notify_msg(verb: &str, names: &[String], color: &str) -> String {
    let reset = "{reset}";
    match names.len() {
        0 => String::new(),
        1 => format!("{color}{verb}: {}{reset}", names[0]),
        n => format!("{color}{verb}: {} items ({}){reset}", n, names[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matchmaker::Action;

    #[test]
    fn test_parse_actions() {
        assert!(Action::<MMAction>::from_str("Unbind(QueryChange)").is_ok());
        assert!(Action::<MMAction>::from_str("Filtering(false)").is_ok());
        assert!(Action::<MMAction>::from_str("SetPrompt(rg> )").is_ok());
        assert!(Action::<MMAction>::from_str("Reload").is_ok());

        let bind_inner = match Action::<MMAction>::from_str(
            "Bind(QueryChange = Reload(rg --column --line-number --no-heading --color=always --smart-case \"$FZF_QUERY\"))",
        )
        .unwrap()
        {
            Action::Custom(MMAction::Bind(s)) => s,
            _ => panic!(),
        };

        let (_trigger, actions) = parse_bind_parts(&bind_inner).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Reload(cmd) => assert_eq!(
                cmd,
                "rg --column --line-number --no-heading --color=always --smart-case \"$FZF_QUERY\""
            ),
            _ => panic!(),
        }

        let push_inner = match Action::<MMAction>::from_str("PushBind(ctrl-r = @enter_mm)").unwrap()
        {
            Action::Custom(MMAction::PushBind(s)) => s,
            _ => panic!(),
        };

        let (_trigger, action) = parse_push_bind_parts(&push_inner).unwrap();
        assert_eq!(action, Action::Semantic("enter_mm".into()));
    }

    #[test]
    fn test_reload_next_and_prev_query_preservation() {
        use matchmaker::Selector;
        use matchmaker::config::*;
        use matchmaker::nucleo::Worker;
        use matchmaker::render::State;
        use matchmaker::ui::{DisplayUI, PickerUI, UI};
        use std::sync::{Arc, Mutex};

        let (bind_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (render_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (controller_tx, _) = tokio::sync::mpsc::unbounded_channel();

        let mut matcher = matchmaker::nucleo::nucleo::Matcher::default();
        let worker = Worker::<ConfigMMItem>::new_indexable(["col0"], None);
        let selection_set: Selector<ConfigMMItem, ConfigMMInnerItem> =
            Selector::new(matchmaker::nucleo::Indexed::identifier).disabled();

        let mut picker_ui = PickerUI::new(
            ResultsConfig::default(),
            StatusConfig::default(),
            QueryConfig::default(),
            DisplayConfig::default(),
            ActionBoxConfig::default(),
            BreadcrumbConfig::default(),
            &mut matcher,
            worker,
            selection_set,
        );

        let mut ui = UI {
            layout: None,
            area: ratatui::layout::Rect::default(),
            config: UiConfig::default(),
        };
        let mut footer_ui = DisplayUI::new(DisplayConfig::default());
        let mut preview_ui = None;
        let mut state = State::new();

        let mut action_context = ActionContext {
            bind_tx,
            render_tx,
            additional_commands: (
                vec!["cmd0".to_string(), "cmd1".to_string(), "cmd2".to_string()],
                0,
            ),
            output_template: None,
            print_handle: AppendOnly::new(),
            output_separator: "\n".to_string(),
            clipboard: Arc::new(Mutex::new(None)),
            fm_notify: false,
            undo_stack: Arc::new(Mutex::new(Vec::new())),
            redo_stack: Arc::new(Mutex::new(Vec::new())),
            fm_action: None,
            mode_history: std::collections::HashMap::new(),
            last_cwd: std::env::current_dir().ok(),
        };

        let mut mm_state = state.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );

        // 1. Initially on index 0 (normal mode)
        mm_state
            .picker_ui
            .query
            .set(Some("normal_query".to_string()), 12);
        assert_eq!(mm_state.picker_ui.query.input, "normal_query");

        // 2. Cycle to index 1 (cycle mode)
        action_handler(
            MMAction::ReloadNext(None),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(action_context.additional_commands.1, 1);
        assert_eq!(mm_state.picker_ui.query.input, "");

        // 3. User types cycle query on index 1
        mm_state
            .picker_ui
            .query
            .set(Some("cycle_query".to_string()), 11);

        // 4. Cycle to index 2
        action_handler(
            MMAction::ReloadNext(None),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(action_context.additional_commands.1, 2);
        assert_eq!(mm_state.picker_ui.query.input, "");

        // 5. Cycle back to index 0 (normal mode)
        action_handler(
            MMAction::ReloadNext(None),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(action_context.additional_commands.1, 0);
        assert_eq!(mm_state.picker_ui.query.input, "normal_query");

        // 6. Cycle to index 1 (cycle mode)
        action_handler(
            MMAction::ReloadNext(None),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(action_context.additional_commands.1, 1);
        assert_eq!(mm_state.picker_ui.query.input, "cycle_query");

        // 7. Cycle backwards (ReloadPrev) to index 0 (normal mode)
        action_handler(MMAction::ReloadPrev, &mut mm_state, &mut action_context);
        assert_eq!(action_context.additional_commands.1, 0);
        assert_eq!(mm_state.picker_ui.query.input, "normal_query");

        // 8. Test nav mode focus behavior
        mm_state.ui.config.nav_mode = true;
        mm_state.focus = matchmaker::render::Focus::Results;
        action_handler(
            MMAction::ReloadNext(None),
            &mut mm_state,
            &mut action_context,
        );
        let mode_0_hist = action_context.mode_history.get(&0).unwrap();
        assert_eq!(mode_0_hist.focus, matchmaker::render::Focus::Results);
        assert_eq!(mm_state.focus, matchmaker::render::Focus::Input);

        // Switch back to mode 0 - should restore Nav mode (Focus::Results)
        action_handler(
            MMAction::ReloadNext(Some(0)),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(action_context.additional_commands.1, 0);
        assert_eq!(mm_state.focus, matchmaker::render::Focus::Results);
    }

    #[test]
    fn test_fm_toggle_pin() {
        use matchmaker::Selector;
        use matchmaker::config::*;
        use matchmaker::nucleo::Worker;
        use matchmaker::render::State;
        use matchmaker::ui::{DisplayUI, PickerUI, UI};
        use std::sync::{Arc, Mutex};

        let (bind_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (render_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (controller_tx, _) = tokio::sync::mpsc::unbounded_channel();

        let mut matcher = matchmaker::nucleo::nucleo::Matcher::default();
        let worker = Worker::<ConfigMMItem>::new_indexable(["col0"], None);
        let selection_set: Selector<ConfigMMItem, ConfigMMInnerItem> =
            Selector::new(matchmaker::nucleo::Indexed::identifier).disabled();

        let mut picker_ui = PickerUI::new(
            ResultsConfig::default(),
            StatusConfig::default(),
            QueryConfig::default(),
            DisplayConfig::default(),
            ActionBoxConfig::default(),
            BreadcrumbConfig::default(),
            &mut matcher,
            worker,
            selection_set,
        );

        let mut ui = UI {
            layout: None,
            area: ratatui::layout::Rect::default(),
            config: UiConfig::default(),
        };
        let mut footer_ui = DisplayUI::new(DisplayConfig::default());
        let mut preview_ui = None;
        let mut state = State::new();

        let mut action_context = ActionContext {
            bind_tx,
            render_tx,
            additional_commands: (vec!["cmd0".to_string()], 0),
            output_template: None,
            print_handle: AppendOnly::new(),
            output_separator: "\n".to_string(),
            clipboard: Arc::new(Mutex::new(None)),
            fm_notify: false,
            undo_stack: Arc::new(Mutex::new(Vec::new())),
            redo_stack: Arc::new(Mutex::new(Vec::new())),
            fm_action: None,
            mode_history: std::collections::HashMap::new(),
            last_cwd: std::env::current_dir().ok(),
        };

        let mut mm_state = state.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );

        let db_file =
            std::env::temp_dir().join(format!("test_frecency_{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&db_file);
        unsafe {
            std::env::set_var("MM_FRECENCY_DB", db_file.to_str().unwrap());
        }

        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let store = matchmaker::frecency::FrecencyStore::open();
        let initial_state = store.is_pinned(&cwd);

        // 1st Toggle
        action_handler(MMAction::FmTogglePin, &mut mm_state, &mut action_context);
        let after_1st = store.is_pinned(&cwd);
        assert_eq!(after_1st, !initial_state);

        // 2nd Toggle
        action_handler(MMAction::FmTogglePin, &mut mm_state, &mut action_context);
        let after_2nd = store.is_pinned(&cwd);
        assert_eq!(after_2nd, initial_state);

        unsafe {
            std::env::remove_var("MM_FRECENCY_DB");
        }
        let _ = std::fs::remove_file(&db_file);
    }

    #[tokio::test]
    async fn test_fm_paste_into_and_flash() {
        use matchmaker::nucleo::injector::Injector;
        use matchmaker::preview::AppendOnly;
        use matchmaker::render::State;
        use matchmaker::ui::UI;
        use std::sync::{Arc, Mutex};
        use tokio::sync::mpsc;

        let (mut mm, injector, _guard) = matchmaker::ConfigMatchmaker::new_from_config(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );

        let temp_dir = std::env::temp_dir().join(format!("test_paste_into_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let sub_dir = temp_dir.join("target_folder");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let source_file = temp_dir.join("source.txt");
        std::fs::write(&source_file, "hello world").unwrap();

        injector
            .push((None, sub_dir.to_string_lossy().to_string()))
            .unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let mut tui =
            matchmaker::tui::Tui::new(matchmaker::config::TerminalConfig::default()).unwrap();
        let mut matcher =
            matchmaker::nucleo::nucleo::Matcher::new(matchmaker::nucleo::nucleo::Config::DEFAULT);

        let hidden_columns = vec![false];
        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            mm.selector,
            None,
            &mut tui,
            hidden_columns,
        );

        let (bind_tx, _) = mpsc::unbounded_channel();
        let (render_tx, _) = mpsc::unbounded_channel();
        let (controller_tx, _) = mpsc::unbounded_channel();

        let clipboard = Arc::new(Mutex::new(Some(crate::fm::FmClipboard {
            items: vec![source_file.clone()],
            op: crate::fm::ClipOp::Copy,
        })));
        let undo_stack = Arc::new(Mutex::new(Vec::new()));
        let redo_stack = Arc::new(Mutex::new(Vec::new()));

        let mut action_context = ActionContext {
            bind_tx,
            render_tx,
            additional_commands: (vec!["cmd0".to_string()], 0),
            output_template: None,
            print_handle: AppendOnly::new(),
            output_separator: "\n".to_string(),
            clipboard: clipboard.clone(),
            fm_notify: false,
            undo_stack: undo_stack.clone(),
            redo_stack: redo_stack.clone(),
            fm_action: None,
            mode_history: std::collections::HashMap::new(),
            last_cwd: Some(temp_dir.clone()),
        };

        let mut mm_state = state_obj.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );

        // Test FmPasteInto
        action_handler(MMAction::FmPasteInto, &mut mm_state, &mut action_context);

        let pasted_file = sub_dir.join("source.txt");
        assert!(
            pasted_file.exists(),
            "Pasted file should exist inside target_folder"
        );
        assert_eq!(
            std::fs::read_to_string(&pasted_file).unwrap(),
            "hello world"
        );
        assert_eq!(
            undo_stack.lock().unwrap().len(),
            1,
            "Undo stack should have 1 item"
        );
        assert!(
            clipboard.lock().unwrap().is_none(),
            "Clipboard should be cleared after paste"
        );
        assert!(
            mm_state.picker_ui.action_visible,
            "Action box should be visible for info notification"
        );

        // Test Flash action
        action_handler(
            MMAction::FmSetFlashCopy("target_folder".to_string()),
            &mut mm_state,
            &mut action_context,
        );
        let flash_op = mm_state
            .picker_ui
            .results
            .get_flash_op("target_folder", &temp_dir);
        assert_eq!(flash_op, Some(matchmaker::ui::results::FlashOp::Copy));

        // Test Undo
        action_handler(MMAction::FmUndo, &mut mm_state, &mut action_context);
        assert!(
            !pasted_file.exists(),
            "Pasted file should be removed after undo"
        );
        assert!(
            clipboard.lock().unwrap().is_some(),
            "Clipboard should be restored after undo"
        );
        assert!(
            mm_state.picker_ui.action_visible,
            "Action box should be visible with undo notification"
        );

        // Test exact item counts in yank_paths and cut_paths
        action_handler(
            MMAction::FmSetYankPaths("folder/\nfile.txt".to_string()),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(
            mm_state.picker_ui.results.yank_paths.len(),
            2,
            "yank_paths should contain exactly 2 items (1 for folder/ and 1 for file.txt)"
        );

        action_handler(
            MMAction::FmRemoveYankPaths("folder/".to_string()),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(
            mm_state.picker_ui.results.yank_paths.len(),
            1,
            "yank_paths should contain exactly 1 item after removing folder/"
        );
        assert!(mm_state.picker_ui.results.yank_paths.contains("file.txt"));

        action_handler(
            MMAction::FmSetCutPaths("single_folder/".to_string()),
            &mut mm_state,
            &mut action_context,
        );
        assert_eq!(
            mm_state.picker_ui.results.cut_paths.len(),
            1,
            "cut_paths should contain exactly 1 item for single folder"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_fm_batch_paste_and_undo_clipboard_restoration() {
        use matchmaker::nucleo::injector::Injector;
        use matchmaker::preview::AppendOnly;
        use matchmaker::render::State;
        use matchmaker::ui::UI;
        use std::sync::{Arc, Mutex};
        use tokio::sync::mpsc;

        let (mut mm, injector, _guard) = matchmaker::ConfigMatchmaker::new_from_config(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );

        let temp_dir =
            std::env::temp_dir().join(format!("test_batch_paste_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let file1 = temp_dir.join("file1.txt");
        let file2 = temp_dir.join("file2.txt");
        let target_dir = temp_dir.join("dest_dir");
        std::fs::write(&file1, "content 1").unwrap();
        std::fs::write(&file2, "content 2").unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();

        injector
            .push((None, target_dir.to_string_lossy().to_string()))
            .unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let mut tui =
            matchmaker::tui::Tui::new(matchmaker::config::TerminalConfig::default()).unwrap();
        let mut matcher =
            matchmaker::nucleo::nucleo::Matcher::new(matchmaker::nucleo::nucleo::Config::DEFAULT);

        let hidden_columns = vec![false];
        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            mm.selector,
            None,
            &mut tui,
            hidden_columns,
        );

        let (bind_tx, _) = mpsc::unbounded_channel();
        let (render_tx, _) = mpsc::unbounded_channel();
        let (controller_tx, _) = mpsc::unbounded_channel();

        let clipboard = Arc::new(Mutex::new(Some(crate::fm::FmClipboard {
            items: vec![file1.clone(), file2.clone()],
            op: crate::fm::ClipOp::Copy,
        })));
        let undo_stack = Arc::new(Mutex::new(Vec::new()));
        let redo_stack = Arc::new(Mutex::new(Vec::new()));

        let mut action_context = ActionContext {
            bind_tx,
            render_tx,
            additional_commands: (vec!["cmd0".to_string()], 0),
            output_template: None,
            print_handle: AppendOnly::new(),
            output_separator: "\n".to_string(),
            clipboard: clipboard.clone(),
            fm_notify: true,
            undo_stack: undo_stack.clone(),
            redo_stack: redo_stack.clone(),
            fm_action: None,
            mode_history: std::collections::HashMap::new(),
            last_cwd: Some(temp_dir.clone()),
        };

        let mut mm_state = state_obj.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );

        mm_state
            .picker_ui
            .results
            .yank_paths
            .insert("file1.txt".to_string());
        mm_state
            .picker_ui
            .results
            .yank_paths
            .insert("file2.txt".to_string());

        // Execute Paste Into target_dir
        action_handler(MMAction::FmPasteInto, &mut mm_state, &mut action_context);

        let pasted1 = target_dir.join("file1.txt");
        let pasted2 = target_dir.join("file2.txt");
        assert!(pasted1.exists(), "pasted1 should exist");
        assert!(pasted2.exists(), "pasted2 should exist");
        assert!(
            clipboard.lock().unwrap().is_none(),
            "clipboard cleared after paste"
        );
        assert!(
            mm_state.picker_ui.results.yank_paths.is_empty(),
            "yank_paths cleared after paste"
        );
        assert!(
            mm_state.picker_ui.action_visible,
            "action box visible for feedback"
        );

        // Execute Undo
        action_handler(MMAction::FmUndo, &mut mm_state, &mut action_context);

        assert!(!pasted1.exists(), "pasted1 should be removed on undo");
        assert!(!pasted2.exists(), "pasted2 should be removed on undo");
        assert!(
            clipboard.lock().unwrap().is_some(),
            "clipboard restored on undo"
        );
        assert_eq!(clipboard.lock().unwrap().as_ref().unwrap().items.len(), 2);
        assert_eq!(
            mm_state.picker_ui.results.yank_paths.len(),
            2,
            "yank_paths restored on undo"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_fm_paste_into_chdir_and_undo_return_and_redo() {
        use matchmaker::action::Action;
        use matchmaker::message::RenderCommand;
        use matchmaker::nucleo::injector::Injector;
        use matchmaker::preview::AppendOnly;
        use matchmaker::render::State;
        use matchmaker::ui::UI;
        use std::sync::{Arc, Mutex};
        use tokio::sync::mpsc;

        let (mut mm, injector, _guard) = matchmaker::ConfigMatchmaker::new_from_config(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );

        let temp_dir =
            std::env::temp_dir().join(format!("test_paste_into_chdir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let cwd = std::env::current_dir().unwrap_or_default();

        let src_file = temp_dir.join("payload.txt");
        let dest_folder = temp_dir.join("target_subfolder");
        std::fs::write(&src_file, "payload data").unwrap();
        std::fs::create_dir_all(&dest_folder).unwrap();

        injector
            .push((None, dest_folder.to_string_lossy().to_string()))
            .unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let mut tui =
            matchmaker::tui::Tui::new(matchmaker::config::TerminalConfig::default()).unwrap();
        let mut matcher =
            matchmaker::nucleo::nucleo::Matcher::new(matchmaker::nucleo::nucleo::Config::DEFAULT);

        let hidden_columns = vec![false];
        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            mm.selector,
            None,
            &mut tui,
            hidden_columns,
        );

        let (bind_tx, _) = mpsc::unbounded_channel();
        let (render_tx, mut render_rx) = mpsc::unbounded_channel();
        let (controller_tx, _) = mpsc::unbounded_channel();

        let clipboard = Arc::new(Mutex::new(Some(crate::fm::FmClipboard {
            items: vec![src_file.clone()],
            op: crate::fm::ClipOp::Copy,
        })));
        let undo_stack = Arc::new(Mutex::new(Vec::new()));
        let redo_stack = Arc::new(Mutex::new(Vec::new()));

        let mut action_context = ActionContext {
            bind_tx,
            render_tx,
            additional_commands: (vec!["cmd0".to_string()], 0),
            output_template: None,
            print_handle: AppendOnly::new(),
            output_separator: "\n".to_string(),
            clipboard: clipboard.clone(),
            fm_notify: true,
            undo_stack: undo_stack.clone(),
            redo_stack: redo_stack.clone(),
            fm_action: None,
            mode_history: std::collections::HashMap::new(),
            last_cwd: Some(cwd.clone()),
        };

        let mut mm_state = state_obj.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );

        // 1. Execute FmPasteInto
        action_handler(MMAction::FmPasteInto, &mut mm_state, &mut action_context);

        let pasted = dest_folder.join("payload.txt");
        assert!(pasted.exists(), "Item should be copied into dest_folder");

        // Check that undo stack recorded previous_dir and target_dir
        {
            let u = undo_stack.lock().unwrap();
            assert_eq!(u.len(), 1);
            if let crate::fm::UndoAction::Paste {
                previous_dir,
                target_dir,
                ..
            } = &u[0]
            {
                assert_eq!(previous_dir.as_ref(), Some(&cwd));
                assert_eq!(target_dir.as_ref(), Some(&dest_folder));
            } else {
                panic!("Expected UndoAction::Paste");
            }
        }

        // Drain render_rx and check that ChDir to dest_folder was sent
        let mut chdir_dest_found = false;
        while let Ok(cmd) = render_rx.try_recv() {
            if let RenderCommand::Action(Action::ChDir(dir)) = cmd {
                if dir == dest_folder.to_string_lossy().to_string() {
                    chdir_dest_found = true;
                }
            }
        }
        assert!(
            chdir_dest_found,
            "FmPasteInto must send Action::ChDir to dest_folder"
        );

        // Check that TARGET_ITEM was set to the pasted file name
        assert_eq!(
            crate::start::TARGET_ITEM.lock().unwrap().as_deref(),
            Some("payload.txt"),
            "TARGET_ITEM should highlight pasted item inside dest_folder"
        );

        // 2. Execute FmUndo
        action_handler(MMAction::FmUndo, &mut mm_state, &mut action_context);
        assert!(!pasted.exists(), "Pasted item should be deleted on undo");

        // Drain render_rx and check that ChDir to previous_dir (cwd) was sent
        let mut chdir_prev_found = false;
        while let Ok(cmd) = render_rx.try_recv() {
            if let RenderCommand::Action(Action::ChDir(dir)) = cmd {
                if dir == cwd.to_string_lossy().to_string() {
                    chdir_prev_found = true;
                }
            }
        }
        assert!(
            chdir_prev_found,
            "FmUndo must send Action::ChDir to previous_dir"
        );

        // Check that TARGET_ITEM was set to dest_folder name so it is selected back in previous_dir
        assert_eq!(
            crate::start::TARGET_ITEM.lock().unwrap().as_deref(),
            Some("target_subfolder"),
            "TARGET_ITEM should highlight target_subfolder when returning to parent on undo"
        );

        // 3. Execute FmRedo
        action_handler(MMAction::FmRedo, &mut mm_state, &mut action_context);
        assert!(pasted.exists(), "Pasted item should be restored on redo");

        let mut chdir_redo_found = false;
        while let Ok(cmd) = render_rx.try_recv() {
            if let RenderCommand::Action(Action::ChDir(dir)) = cmd {
                if dir == dest_folder.to_string_lossy().to_string() {
                    chdir_redo_found = true;
                }
            }
        }
        assert!(
            chdir_redo_found,
            "FmRedo must send Action::ChDir to dest_folder"
        );

        assert_eq!(
            crate::start::TARGET_ITEM.lock().unwrap().as_deref(),
            Some("payload.txt"),
            "TARGET_ITEM should highlight pasted item inside dest_folder on redo"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_fm_paste_into_cut_and_undo_return_and_redo() {
        use matchmaker::action::Action;
        use matchmaker::message::RenderCommand;
        use matchmaker::nucleo::injector::Injector;
        use matchmaker::preview::AppendOnly;
        use matchmaker::render::State;
        use matchmaker::ui::UI;
        use std::sync::{Arc, Mutex};
        use tokio::sync::mpsc;

        let (mut mm, injector, _guard) = matchmaker::ConfigMatchmaker::new_from_config(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );

        let temp_dir =
            std::env::temp_dir().join(format!("test_paste_into_cut_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let cwd = std::env::current_dir().unwrap_or_default();

        let src_file = temp_dir.join("cut_payload.txt");
        let dest_folder = temp_dir.join("cut_target_subfolder");
        std::fs::write(&src_file, "cut payload data").unwrap();
        std::fs::create_dir_all(&dest_folder).unwrap();

        injector
            .push((None, dest_folder.to_string_lossy().to_string()))
            .unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let mut tui =
            matchmaker::tui::Tui::new(matchmaker::config::TerminalConfig::default()).unwrap();
        let mut matcher =
            matchmaker::nucleo::nucleo::Matcher::new(matchmaker::nucleo::nucleo::Config::DEFAULT);

        let hidden_columns = vec![false];
        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            mm.selector,
            None,
            &mut tui,
            hidden_columns,
        );

        let (bind_tx, _) = mpsc::unbounded_channel();
        let (render_tx, mut render_rx) = mpsc::unbounded_channel();
        let (controller_tx, _) = mpsc::unbounded_channel();

        let clipboard = Arc::new(Mutex::new(Some(crate::fm::FmClipboard {
            items: vec![src_file.clone()],
            op: crate::fm::ClipOp::Cut,
        })));
        let undo_stack = Arc::new(Mutex::new(Vec::new()));
        let redo_stack = Arc::new(Mutex::new(Vec::new()));

        let mut action_context = ActionContext {
            bind_tx,
            render_tx,
            additional_commands: (vec!["cmd0".to_string()], 0),
            output_template: None,
            print_handle: AppendOnly::new(),
            output_separator: "\n".to_string(),
            clipboard: clipboard.clone(),
            fm_notify: true,
            undo_stack: undo_stack.clone(),
            redo_stack: redo_stack.clone(),
            fm_action: None,
            mode_history: std::collections::HashMap::new(),
            last_cwd: Some(cwd.clone()),
        };

        let mut mm_state = state_obj.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );

        // 1. Execute FmPasteInto (Cut)
        action_handler(MMAction::FmPasteInto, &mut mm_state, &mut action_context);

        let moved_file = dest_folder.join("cut_payload.txt");
        assert!(moved_file.exists(), "Item should be moved into dest_folder");
        assert!(
            !src_file.exists(),
            "Source file should no longer exist at original path"
        );

        // Drain render_rx and check that ChDir to dest_folder was sent
        let mut chdir_dest_found = false;
        while let Ok(cmd) = render_rx.try_recv() {
            if let RenderCommand::Action(Action::ChDir(dir)) = cmd {
                if dir == dest_folder.to_string_lossy().to_string() {
                    chdir_dest_found = true;
                }
            }
        }
        assert!(
            chdir_dest_found,
            "FmPasteInto must send Action::ChDir to dest_folder"
        );
        assert_eq!(
            crate::start::TARGET_ITEM.lock().unwrap().as_deref(),
            Some("cut_payload.txt"),
            "TARGET_ITEM should highlight moved item inside dest_folder"
        );

        // 2. Execute FmUndo
        action_handler(MMAction::FmUndo, &mut mm_state, &mut action_context);
        assert!(
            !moved_file.exists(),
            "Moved file should no longer be in dest_folder"
        );
        assert!(
            src_file.exists(),
            "Source file should be moved back to original path"
        );

        let mut chdir_prev_found = false;
        while let Ok(cmd) = render_rx.try_recv() {
            if let RenderCommand::Action(Action::ChDir(dir)) = cmd {
                if dir == cwd.to_string_lossy().to_string() {
                    chdir_prev_found = true;
                }
            }
        }
        assert!(
            chdir_prev_found,
            "FmUndo must send Action::ChDir to previous_dir"
        );
        assert_eq!(
            crate::start::TARGET_ITEM.lock().unwrap().as_deref(),
            Some("cut_target_subfolder"),
            "TARGET_ITEM should highlight cut_target_subfolder when returning to parent on undo"
        );

        // 3. Execute FmRedo
        action_handler(MMAction::FmRedo, &mut mm_state, &mut action_context);
        assert!(
            moved_file.exists(),
            "File should be moved into dest_folder again on redo"
        );
        assert!(
            !src_file.exists(),
            "Source file should not exist at original path on redo"
        );

        let mut chdir_redo_found = false;
        while let Ok(cmd) = render_rx.try_recv() {
            if let RenderCommand::Action(Action::ChDir(dir)) = cmd {
                if dir == dest_folder.to_string_lossy().to_string() {
                    chdir_redo_found = true;
                }
            }
        }
        assert!(
            chdir_redo_found,
            "FmRedo must send Action::ChDir to dest_folder"
        );
        assert_eq!(
            crate::start::TARGET_ITEM.lock().unwrap().as_deref(),
            Some("cut_payload.txt"),
            "TARGET_ITEM should highlight moved item inside dest_folder on redo"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
