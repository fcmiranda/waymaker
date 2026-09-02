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
    pub query_history: std::collections::HashMap<usize, (String, usize)>,
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
        query_history,
    }: &mut ActionContext,
) {
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
                query_history.insert(
                    old_index,
                    (
                        state.picker_ui.query.input.clone(),
                        state.picker_ui.query.cursor,
                    ),
                );
                if let Some((saved_input, saved_cursor)) = query_history.get(&index) {
                    state
                        .picker_ui
                        .query
                        .set(Some(saved_input.clone()), *saved_cursor as u16);
                } else {
                    state.picker_ui.query.set(Some(String::new()), 0);
                }
            }

            let payload = &additional_commands.0[index];
            state.envs.set("MM_INDEX", index);
            state.set_interrupt(Interrupt::Reload, payload.clone());
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
                query_history.insert(
                    old_index,
                    (
                        state.picker_ui.query.input.clone(),
                        state.picker_ui.query.cursor,
                    ),
                );
                if let Some((saved_input, saved_cursor)) = query_history.get(&index) {
                    state
                        .picker_ui
                        .query
                        .set(Some(saved_input.clone()), *saved_cursor as u16);
                } else {
                    state.picker_ui.query.set(Some(String::new()), 0);
                }
            }

            let payload = &additional_commands.0[index];

            state.envs.set("MM_INDEX", index);

            state.set_interrupt(Interrupt::Reload, payload.clone());
        }

        MMAction::ReloadReady(_) => {
            state.reloading = false;
            state.picker_ui.update();
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
            let cwd = std::env::current_dir().unwrap_or_default();
            state.picker_ui.results.yank_paths = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let path = PathBuf::from(&s);
                    if path.is_absolute() {
                        s
                    } else {
                        cwd.join(path).to_string_lossy().to_string()
                    }
                })
                .collect();
        }
        MMAction::FmRemoveYankPaths(raw) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let to_remove: std::collections::HashSet<String> = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let path = PathBuf::from(&s);
                    if path.is_absolute() {
                        s
                    } else {
                        cwd.join(path).to_string_lossy().to_string()
                    }
                })
                .collect();
            for item in to_remove {
                state.picker_ui.results.yank_paths.remove(&item);
            }
        }
        MMAction::FmRemoveCutPaths(raw) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let to_remove: std::collections::HashSet<String> = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let path = PathBuf::from(&s);
                    if path.is_absolute() {
                        s
                    } else {
                        cwd.join(path).to_string_lossy().to_string()
                    }
                })
                .collect();
            for item in to_remove {
                state.picker_ui.results.cut_paths.remove(&item);
            }
        }
        MMAction::FmSetCutPaths(raw) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            state.picker_ui.results.cut_paths = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let path = PathBuf::from(&s);
                    if path.is_absolute() {
                        s
                    } else {
                        cwd.join(path).to_string_lossy().to_string()
                    }
                })
                .collect();
        }
        MMAction::FmSetPinPaths(raw) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            state.picker_ui.results.pin_paths = raw
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let path = PathBuf::from(&s);
                    if path.is_absolute() {
                        s
                    } else {
                        cwd.join(path).to_string_lossy().to_string()
                    }
                })
                .collect();
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
                    MMAction::FmSetYankPaths(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetCutPaths(String::new()),
                )));
                if *fm_notify {
                    let msg = fm_notify_msg("Copied", &items, "{green}");
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
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
                    let msg = fm_notify_msg("Un-yanked", &items, "{green}");
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
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
                    let msg = fm_notify_msg("Un-cut", &items, "{yellow}");
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
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
                    MMAction::FmSetCutPaths(items.join("\n")),
                )));
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::FmSetYankPaths(String::new()),
                )));
                if *fm_notify {
                    let msg = fm_notify_msg("Cut", &items, "{yellow}");
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
                }
            }
        }
        MMAction::FmPaste => {
            let clip = clipboard.lock().ok().and_then(|g| g.clone());
            if let Some(clip) = clip {
                let cwd = std::env::current_dir().unwrap_or_default();
                let mut had_error = false;
                for src in &clip.items {
                    let result = match clip.op {
                        crate::fm::ClipOp::Copy => crate::fm::copy_into(src, &cwd),
                        crate::fm::ClipOp::Cut => crate::fm::move_into(src, &cwd),
                    };
                    if let Err(e) = result {
                        error!("fm paste '{}': {e}", src.display());
                        had_error = true;
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
                }
                if *fm_notify {
                    let names: Vec<String> = clip
                        .items
                        .iter()
                        .filter_map(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .collect();
                    let verb = match clip.op {
                        crate::fm::ClipOp::Copy => "Pasted",
                        crate::fm::ClipOp::Cut => "Moved",
                    };
                    let color = if had_error { "{red}" } else { "{cyan}" };
                    let msg = fm_notify_msg(verb, &names, color);
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
                }
                let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
            }
        }
        MMAction::FmUndo => {
            let action = undo_stack.lock().ok().and_then(|mut s| s.pop());
            if let Some(action) = action {
                if let Ok(mut rs) = redo_stack.lock() {
                    rs.push(action.clone());
                }
                if let Err(e) = crate::fm::apply_undo(&action) {
                    error!("fm undo: {e}");
                }
                let _ = render_tx.send(RenderCommand::Action(Action::Reload(String::new())));
            }
        }
        MMAction::FmRedo => {
            let action = redo_stack.lock().ok().and_then(|mut s| s.pop());
            if let Some(action) = action {
                let redo_action = match &action {
                    crate::fm::UndoAction::DeletedFile { original, backup } => {
                        crate::fm::UndoAction::DeletedFile {
                            original: backup.clone(),
                            backup: original.clone(),
                        }
                    }
                    crate::fm::UndoAction::CreatedFile { path } => {
                        crate::fm::UndoAction::CreatedFile { path: path.clone() }
                    }
                    crate::fm::UndoAction::Renamed { from, to } => crate::fm::UndoAction::Renamed {
                        from: to.clone(),
                        to: from.clone(),
                    },
                    crate::fm::UndoAction::Copied { dest } => {
                        crate::fm::UndoAction::Copied { dest: dest.clone() }
                    }
                    crate::fm::UndoAction::Moved { from, to } => crate::fm::UndoAction::Moved {
                        from: to.clone(),
                        to: from.clone(),
                    },
                };
                if let Err(e) = crate::fm::apply_undo(&redo_action) {
                    error!("fm redo: {e}");
                } else if let Ok(mut us) = undo_stack.lock() {
                    us.push(redo_action);
                }
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
                            state.picker_ui.results.pin_paths.insert(key_str.clone());
                            state.picker_ui.results.pin_paths.insert(p.clone());
                        } else {
                            state.picker_ui.results.pin_paths.remove(&key_str);
                            state.picker_ui.results.pin_paths.remove(p);
                        }
                    }
                }
                if *fm_notify {
                    let verb = if last_state { "Pinned" } else { "Unpinned" };
                    let color = if last_state { "{yellow:📌}" } else { "{darkgray}" };
                    let msg = fm_notify_msg(verb, &paths, color);
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
                }
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
    CycleSort, HistoryUp, HistoryDown, Accept, ReloadPrev, FmCreateStart, FmDeleteStart, FmRenameStart, FmUnzipStart, FmZipStart, FmYank, FmUnyank, FmCut, FmUncut, FmPaste, FmUndo, FmRedo, FmDragDrop, FmTogglePin;


    tuples:
    Bind, Unbind, PushBind, PopBind, ExecuteOrConfirm, ExecuteAndQuit, BecomeOr, Transform, TransformConfig, SetStyledPrompt, SetStyledStatus, PushHeader, PushFooter, RunPreview, FmSetYankPaths, FmRemoveYankPaths, FmSetCutPaths, FmRemoveCutPaths, FmSetPinPaths, Confirm, Prompt;

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
                        let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                            MMAction::SetStyledStatus(msg),
                        )));
                    }
                }
            }
        }
        FmActionMode::Delete { paths } => {
            let mut deleted = Vec::new();
            for path in &paths {
                let path_buf = PathBuf::from(path);
                match crate::fm::move_to_trash(&path_buf) {
                    Ok(backup) => {
                        deleted.push(path.clone());
                        if let Ok(mut stack) = undo_stack.lock() {
                            stack.push(crate::fm::UndoAction::DeletedFile {
                                original: path_buf,
                                backup,
                            });
                        }
                    }
                    Err(e) => error!("fm delete '{}': {e}", path),
                }
            }
            if fm_notify && !deleted.is_empty() {
                let msg = fm_notify_msg("Deleted", &deleted, "{red}");
                let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                    MMAction::SetStyledStatus(msg),
                )));
            }
        }
        FmActionMode::Rename { from, remaining } => {
            if !input.is_empty() && input != from {
                if let Err(e) =
                    crate::fm::move_path(std::path::Path::new(&from), std::path::Path::new(&input))
                {
                    error!("fm rename '{}' -> '{input}': {e}", from);
                } else {
                    if let Ok(mut stack) = undo_stack.lock() {
                        stack.push(crate::fm::UndoAction::Renamed {
                            from: PathBuf::from(&from),
                            to: PathBuf::from(&input),
                        });
                    }
                    if fm_notify {
                        let msg = format!("{{cyan:Renamed:}} {} -> {}", from, input);
                        let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                            MMAction::SetStyledStatus(msg),
                        )));
                    }
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
        }
        FmActionMode::Unzip { src } => {
            if !input.is_empty() {
                if let Err(e) = std::fs::create_dir_all(&input) {
                    error!("fm unzip: create dir '{input}': {e}");
                } else if let Err(e) = crate::fm::extract_archive(&src, &input) {
                    error!("fm unzip '{src}' -> '{input}': {e}");
                } else if fm_notify {
                    let msg = format!("{{green:Extracted:}} {} -> {}", src, input);
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
                }
            }
        }
        FmActionMode::Zip { paths } => {
            if !input.is_empty() {
                if let Err(e) = crate::fm::create_archive(&input, &paths) {
                    error!("fm zip '{input}': {e}");
                } else if fm_notify {
                    let msg = format!("{{green:Compressed:}} {}", input);
                    let _ = render_tx.send(RenderCommand::Action(Action::Custom(
                        MMAction::SetStyledStatus(msg),
                    )));
                }
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
        }
    }

    close_action_box(state, fm_action);
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
        use matchmaker::config::*;
        use matchmaker::nucleo::Worker;
        use matchmaker::render::State;
        use matchmaker::ui::{DisplayUI, PickerUI, UI};
        use matchmaker::Selector;
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
                vec![
                    "cmd0".to_string(),
                    "cmd1".to_string(),
                    "cmd2".to_string(),
                ],
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
            query_history: std::collections::HashMap::new(),
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
    }

    #[test]
    fn test_fm_toggle_pin() {
        use matchmaker::config::*;
        use matchmaker::nucleo::Worker;
        use matchmaker::render::State;
        use matchmaker::ui::{DisplayUI, PickerUI, UI};
        use matchmaker::Selector;
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
            query_history: std::collections::HashMap::new(),
        };

        let mut mm_state = state.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );

        let cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
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
    }
}
