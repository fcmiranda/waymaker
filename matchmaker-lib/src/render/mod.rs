mod dynamic;
mod state;

use cba::bait::ResultExt;
use crossterm::event::{MouseButton, MouseEventKind};
pub use dynamic::*;
pub use state::*;
// ------------------------------

use std::io::Write;

use log::{debug, info, warn};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use tokio::sync::mpsc;

#[cfg(feature = "bracketed-paste")]
use crate::PasteHandler;
use crate::action::{Action, ActionExt, NullActionExt};
use crate::config::{CursorSetting, ExitConfig, RowConnectionStyle};
use crate::event::{BindSender, EventSender};
use crate::message::{BindDirective, Event, Interrupt, RenderCommand};
use crate::tui::Tui;
use crate::ui::{DisplayUI, OverlayUI, PickerUI, PreviewUI, QueryUI, ResultsUI, UI};
use crate::{ActionAliaser, ActionExtHandler, Initializer, MatchError, SSS, Selection};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};

fn action_from_null<A: ActionExt>(action: Action<NullActionExt>) -> Option<Action<A>> {
    Some(match action {
        Action::Select => Action::Select,
        Action::Deselect => Action::Deselect,
        Action::DeselectUp => Action::DeselectUp,
        Action::Toggle => Action::Toggle,
        Action::ToggleUp => Action::ToggleUp,
        Action::CycleAll => Action::CycleAll,
        Action::ClearSelections => Action::ClearSelections,
        Action::Accept => Action::Accept,
        Action::Quit(x) => Action::Quit(x),
        Action::ToggleWrap => Action::ToggleWrap,
        Action::ToggleActionBox => Action::ToggleActionBox,
        Action::ToggleFocus => Action::ToggleFocus,
        Action::FocusFilter => Action::FocusFilter,
        Action::FocusNav => Action::FocusNav,
        Action::ToggleParentPeek => Action::ToggleParentPeek,
        Action::ToggleFooter => Action::ToggleFooter,
        Action::ToggleHeader => Action::ToggleHeader,
        Action::Up(x) => Action::Up(x),
        Action::Down(x) => Action::Down(x),
        Action::Pos(x) => Action::Pos(x),
        Action::HalfPageDown => Action::HalfPageDown,
        Action::HalfPageUp => Action::HalfPageUp,
        Action::HScroll(x) => Action::HScroll(x),
        Action::VScroll(x) => Action::VScroll(x),
        Action::CyclePreview => Action::CyclePreview,
        Action::Preview(x) => Action::Preview(x),
        Action::Help(x) => Action::Help(x),
        Action::SetPreview(x) => Action::SetPreview(x),
        Action::SwitchPreview(x) => Action::SwitchPreview(x),
        Action::TogglePreviewWrap => Action::TogglePreviewWrap,
        Action::PreviewUp(x) => Action::PreviewUp(x),
        Action::PreviewDown(x) => Action::PreviewDown(x),
        Action::ExpandPreview(x) => Action::ExpandPreview(x),
        Action::ShrinkPreview(x) => Action::ShrinkPreview(x),
        Action::PreviewZoomIn => Action::PreviewZoomIn,
        Action::PreviewZoomOut => Action::PreviewZoomOut,
        Action::PreviewHalfPageUp => Action::PreviewHalfPageUp,
        Action::PreviewHalfPageDown => Action::PreviewHalfPageDown,
        Action::PreviewHScroll(x) => Action::PreviewHScroll(x),
        Action::PreviewScroll(x) => Action::PreviewScroll(x),
        Action::PreviewJump => Action::PreviewJump,
        Action::NextDiagram => Action::NextDiagram,
        Action::PrevDiagram => Action::PrevDiagram,
        Action::DiagramZoomIn => Action::DiagramZoomIn,
        Action::DiagramZoomOut => Action::DiagramZoomOut,
        Action::DiagramResetZoom => Action::DiagramResetZoom,
        Action::PreviewResetZoom => Action::PreviewResetZoom,
        Action::ToggleDiagram => Action::ToggleDiagram,
        Action::NextColumn => Action::NextColumn,
        Action::PrevColumn => Action::PrevColumn,
        Action::SwitchColumn(x) => Action::SwitchColumn(x),
        Action::ToggleColumn(x) => Action::ToggleColumn(x),
        Action::ShowColumn(x) => Action::ShowColumn(x),
        Action::Execute(x) => Action::Execute(x),
        Action::ExecuteAsync(x) => Action::ExecuteAsync(x),
        Action::ExecuteThen(x) => Action::ExecuteThen(x),
        Action::ExecuteSilent(x) => Action::ExecuteSilent(x),
        Action::Become(x) => Action::Become(x),
        Action::BecomeSilent(x) => Action::BecomeSilent(x),
        Action::Reload(x) => Action::Reload(x),
        Action::ChDir(x) => Action::ChDir(x),
        Action::Print(x) => Action::Print(x),
        Action::PrintKey => Action::PrintKey,
        Action::Store(x) => Action::Store(x),
        Action::ForwardChar => Action::ForwardChar,
        Action::BackwardChar => Action::BackwardChar,
        Action::ForwardWord => Action::ForwardWord,
        Action::BackwardWord => Action::BackwardWord,
        Action::DeleteChar => Action::DeleteChar,
        Action::DeleteWord => Action::DeleteWord,
        Action::DeleteNextChar => Action::DeleteNextChar,
        Action::DeleteNextWord => Action::DeleteNextWord,
        Action::DeleteLineStart => Action::DeleteLineStart,
        Action::DeleteLineEnd => Action::DeleteLineEnd,
        Action::Cancel => Action::Cancel,
        Action::SetQuery(x) => Action::SetQuery(x),
        Action::QueryPos(x) => Action::QueryPos(x),
        Action::Char(x) => Action::Char(x),
        Action::Redraw => Action::Redraw,
        Action::Custom(_) => return None,
        Action::Overlay(x) => Action::Overlay(x),
        Action::Semantic(x) => Action::Semantic(x),
        Action::SetMode(x) => Action::SetMode(x),
        Action::Copy(x) => Action::Copy(x),
        Action::CopyAsync(x) => Action::CopyAsync(x),
        Action::Trace(x) => Action::Trace(x),
        Action::SortMenu => Action::SortMenu,
        Action::Sort(x) => Action::Sort(x),
    })
}

/// Pre-process `buffer` when the sort menu is active: intercept the next key to select a sort order or cancel.
fn apply_sort_menu<A: ActionExt>(buffer: &mut Vec<RenderCommand<A>>, sort_menu_active: &mut bool) {
    if !*sort_menu_active {
        return;
    }

    let mut out = Vec::with_capacity(buffer.len());

    for cmd in buffer.drain(..) {
        match cmd {
            RenderCommand::Tick => {
                out.push(RenderCommand::Tick);
            }
            RenderCommand::Action(Action::Char(c))
            | RenderCommand::KeyAction {
                action: Action::Char(c),
                ..
            } => {
                *sort_menu_active = false;
                match c {
                    'a' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::Alphabetical,
                    )))),
                    'A' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::AlphabeticalReverse,
                    )))),
                    'n' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::Natural,
                    )))),
                    'N' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::NaturalReverse,
                    )))),
                    'm' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::Modified,
                    )))),
                    'M' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::ModifiedReverse,
                    )))),
                    'b' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::Created,
                    )))),
                    'B' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::CreatedReverse,
                    )))),
                    's' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::Size,
                    )))),
                    'S' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::SizeReverse,
                    )))),
                    'e' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::Extension,
                    )))),
                    'E' => out.push(RenderCommand::Action(Action::Sort(Some(
                        crate::action::SortOrder::ExtensionReverse,
                    )))),
                    _ => {
                        // Any other char cancels sort menu
                    }
                }
            }
            RenderCommand::Action(Action::Quit(1))
            | RenderCommand::Action(Action::Cancel)
            | RenderCommand::KeyAction {
                action: Action::Quit(1) | Action::Cancel,
                ..
            } => {
                // Escape or Cancel closes sort menu without exiting the picker
                *sort_menu_active = false;
            }
            RenderCommand::KeyAction { key, .. } if key.eq_ignore_ascii_case("esc") => {
                // Escape closes sort menu without exiting the picker
                *sort_menu_active = false;
            }
            RenderCommand::Action(Action::SortMenu)
            | RenderCommand::KeyAction {
                action: Action::SortMenu,
                ..
            } => {
                *sort_menu_active = false;
            }
            RenderCommand::Action(a) | RenderCommand::KeyAction { action: a, .. } => {
                *sort_menu_active = false;
                out.push(RenderCommand::Action(a));
            }
            other => {
                *sort_menu_active = false;
                out.push(other);
            }
        }
    }

    *buffer = out;
}

fn get_nav_bind<'a>(
    focus_binds: &'a std::collections::HashMap<String, crate::action::Actions<NullActionExt>>,
    key: &str,
) -> Option<&'a crate::action::Actions<NullActionExt>> {
    if let Some(actions) = focus_binds.get(key) {
        return Some(actions);
    }
    if key.eq_ignore_ascii_case("space") {
        if let Some(actions) = focus_binds.get(" ") {
            return Some(actions);
        }
    } else if key == " " {
        if let Some(actions) = focus_binds
            .get("space")
            .or_else(|| focus_binds.get("Space"))
        {
            return Some(actions);
        }
    }
    // For multi-character keys (e.g. "Esc", "Backspace", "Tab", "Ctrl-l", "Left", "Right"),
    // check case-insensitively since crokey capitalizes key names while config/users often use lowercase.
    if key.chars().count() > 1 {
        let lower = key.to_ascii_lowercase();
        if let Some(actions) = focus_binds.get(&lower) {
            return Some(actions);
        }
        return focus_binds
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, actions)| actions);
    }
    None
}

fn update_sim_focus<A: ActionExt>(action: &Action<A>, sim_focus: &mut Focus) {
    match action {
        Action::ToggleFocus => {
            *sim_focus = match *sim_focus {
                Focus::Input => Focus::Results,
                Focus::Results => Focus::Input,
            };
        }
        Action::FocusFilter => {
            *sim_focus = Focus::Input;
        }
        Action::FocusNav => {
            *sim_focus = Focus::Results;
        }
        _ => {}
    }
}

fn process_results_nav_key<A: ActionExt>(
    key: &str,
    fallback_action: Option<Action<A>>,
    focus_binds: &std::collections::HashMap<String, crate::action::Actions<NullActionExt>>,
    pending_nav_key: &mut Option<char>,
    sort_menu_active: &mut bool,
    sim_focus: &mut Focus,
    out: &mut Vec<RenderCommand<A>>,
    preview_fullscreen: bool,
) {
    if preview_fullscreen {
        match key {
            "j" | "down" => {
                out.push(RenderCommand::Action(Action::PreviewDown(3)));
                return;
            }
            "k" | "up" => {
                out.push(RenderCommand::Action(Action::PreviewUp(3)));
                return;
            }
            "h" | "left" => {
                out.push(RenderCommand::Action(Action::PreviewHScroll(-6)));
                return;
            }
            "l" | "right" => {
                out.push(RenderCommand::Action(Action::PreviewHScroll(6)));
                return;
            }
            "J" | "shift-j" | "shift-down" => {
                out.push(RenderCommand::Action(Action::PreviewDown(15)));
                return;
            }
            "K" | "shift-k" | "shift-up" => {
                out.push(RenderCommand::Action(Action::PreviewUp(15)));
                return;
            }
            "H" | "shift-left" => {
                out.push(RenderCommand::Action(Action::PreviewHScroll(-18)));
                return;
            }
            "L" | "shift-right" => {
                out.push(RenderCommand::Action(Action::PreviewHScroll(18)));
                return;
            }
            "ctrl-u" | "u" => {
                out.push(RenderCommand::Action(Action::PreviewHalfPageUp));
                return;
            }
            "ctrl-d" | "d" => {
                out.push(RenderCommand::Action(Action::PreviewHalfPageDown));
                return;
            }
            "g" => {
                if *pending_nav_key == Some('g') {
                    *pending_nav_key = None;
                    out.push(RenderCommand::Action(Action::PreviewUp(0)));
                } else {
                    *pending_nav_key = Some('g');
                }
                return;
            }
            "G" => {
                *pending_nav_key = None;
                out.push(RenderCommand::Action(Action::PreviewDown(0)));
                return;
            }
            "s" | "m" => {
                out.push(RenderCommand::Action(Action::ToggleDiagram));
                return;
            }
            "n" | "]" => {
                out.push(RenderCommand::Action(Action::NextDiagram));
                return;
            }
            "N" | "[" => {
                out.push(RenderCommand::Action(Action::PrevDiagram));
                return;
            }
            "+" | "=" => {
                out.push(RenderCommand::Action(Action::DiagramZoomIn));
                return;
            }
            "-" => {
                out.push(RenderCommand::Action(Action::DiagramZoomOut));
                return;
            }
            "0" | "z" => {
                out.push(RenderCommand::Action(Action::DiagramResetZoom));
                return;
            }
            "y" => {
                out.push(RenderCommand::Action(Action::Accept));
                return;
            }
            "ctrl-c" => {
                out.push(RenderCommand::Action(Action::Quit(130)));
                return;
            }
            k if k.eq_ignore_ascii_case("esc") || k == "enter" => {
                *sim_focus = Focus::Input;
                out.push(RenderCommand::Action(Action::CyclePreview));
                return;
            }
            k => {
                *pending_nav_key = None;
                if let Some(actions) = get_nav_bind(focus_binds, k) {
                    for action in actions.iter().cloned() {
                        if let Some(action) = action_from_null::<A>(action) {
                            update_sim_focus(&action, sim_focus);
                            out.push(RenderCommand::Action(action));
                        }
                    }
                }
                return;
            }
        }
    } else {
        // Non-fullscreen preview scrolling and diagram controls fallback
        if (key == "J" || key == "shift-j")
            && !focus_binds.contains_key("J")
            && !focus_binds.contains_key("shift-j")
        {
            out.push(RenderCommand::Action(Action::PreviewDown(5)));
            return;
        }
        if (key == "K" || key == "shift-k")
            && !focus_binds.contains_key("K")
            && !focus_binds.contains_key("shift-k")
        {
            out.push(RenderCommand::Action(Action::PreviewUp(5)));
            return;
        }
        if (key == "ctrl-u" || key == "u")
            && !focus_binds.contains_key("ctrl-u")
            && !focus_binds.contains_key("u")
        {
            out.push(RenderCommand::Action(Action::PreviewHalfPageUp));
            return;
        }
        if (key == "ctrl-d" || key == "d")
            && !focus_binds.contains_key("ctrl-d")
            && !focus_binds.contains_key("d")
        {
            out.push(RenderCommand::Action(Action::PreviewHalfPageDown));
            return;
        }
        if (key == "s" || key == "m")
            && !focus_binds.contains_key("s")
            && !focus_binds.contains_key("m")
        {
            out.push(RenderCommand::Action(Action::ToggleDiagram));
            return;
        }
    }

    if key == "," {
        *pending_nav_key = None;
        *sort_menu_active = true;
        out.push(RenderCommand::Action(Action::SortMenu));
        return;
    }
    match pending_nav_key.take() {
        Some('g') => {
            if key == "g" {
                let seq = "gg".to_string();
                if let Some(actions) = get_nav_bind(focus_binds, &seq) {
                    for action in actions.iter().cloned() {
                        if let Some(action) = action_from_null::<A>(action) {
                            update_sim_focus(&action, sim_focus);
                            out.push(RenderCommand::Action(action));
                        }
                    }
                } else {
                    out.push(RenderCommand::Action(Action::PreviewUp(0)));
                }
            } else if key == "t" {
                let seq = "gt".to_string();
                if let Some(actions) = get_nav_bind(focus_binds, &seq) {
                    for action in actions.iter().cloned() {
                        if let Some(action) = action_from_null::<A>(action) {
                            update_sim_focus(&action, sim_focus);
                            out.push(RenderCommand::Action(action));
                        }
                    }
                } else {
                    out.push(RenderCommand::Action(Action::Pos(0)));
                }
            } else if key == "b" {
                let seq = "gb".to_string();
                if let Some(actions) = get_nav_bind(focus_binds, &seq) {
                    for action in actions.iter().cloned() {
                        if let Some(action) = action_from_null::<A>(action) {
                            update_sim_focus(&action, sim_focus);
                            out.push(RenderCommand::Action(action));
                        }
                    }
                } else {
                    out.push(RenderCommand::Action(Action::Pos(-1)));
                }
            } else if let Some(actions) = get_nav_bind(focus_binds, key) {
                for action in actions.iter().cloned() {
                    if let Some(action) = action_from_null::<A>(action) {
                        update_sim_focus(&action, sim_focus);
                        out.push(RenderCommand::Action(action));
                    }
                }
            }
        }
        _ => {
            if key == "g" {
                *pending_nav_key = Some('g');
            } else if let Some(actions) = get_nav_bind(focus_binds, key) {
                for action in actions.iter().cloned() {
                    if let Some(action) = action_from_null::<A>(action) {
                        update_sim_focus(&action, sim_focus);
                        out.push(RenderCommand::Action(action));
                    }
                }
            } else if let Some(action) = fallback_action {
                match &action {
                    Action::ToggleFocus => {
                        if key.eq_ignore_ascii_case("esc") {
                            out.push(RenderCommand::Action(Action::Quit(130)));
                        } else {
                            *sim_focus = Focus::Input;
                            out.push(RenderCommand::Action(Action::ToggleFocus));
                        }
                    }
                    Action::FocusFilter => {
                        *sim_focus = Focus::Input;
                        out.push(RenderCommand::Action(Action::FocusFilter));
                    }
                    Action::FocusNav => {
                        *sim_focus = Focus::Results;
                        out.push(RenderCommand::Action(Action::FocusNav));
                    }
                    Action::Char(_) => {
                        // Drop unbound plain characters in nav mode
                    }
                    Action::DeleteChar
                    | Action::DeleteWord
                    | Action::DeleteNextChar
                    | Action::DeleteNextWord
                    | Action::DeleteLineStart
                    | Action::DeleteLineEnd => {
                        // Drop unbound editing actions in nav mode
                    }
                    _ => {
                        update_sim_focus(&action, sim_focus);
                        out.push(RenderCommand::Action(action));
                    }
                }
            }
        }
    }
}

/// Pre-process `buffer` for navigation mode: simulate `ToggleFocus` events encountered in the
/// batch and expand `Action::Char` events into `nav_binds` actions while focus is on results.
fn apply_focus_binds<A: ActionExt>(
    buffer: &mut Vec<RenderCommand<A>>,
    initial_focus: Focus,
    focus_binds: &std::collections::HashMap<String, crate::action::Actions<NullActionExt>>,
    overlay_active: bool,
    pending_nav_key: &mut Option<char>,
    sort_menu_active: &mut bool,
    preview_fullscreen: bool,
) {
    if overlay_active {
        for cmd in buffer {
            if let RenderCommand::KeyAction { action, .. } = cmd {
                *cmd = RenderCommand::Action(action.clone());
            }
        }
        return;
    }

    if *sort_menu_active {
        apply_sort_menu(buffer, sort_menu_active);
        return;
    }

    let mut out = Vec::with_capacity(buffer.len());
    let mut sim_focus = if preview_fullscreen {
        Focus::Results
    } else {
        initial_focus
    };
    let mut last_consumed_nav_key: Option<String> = None;

    for cmd in buffer.drain(..) {
        match cmd {
            RenderCommand::Tick => {
                last_consumed_nav_key = None;
                out.push(RenderCommand::Tick);
            }
            RenderCommand::Action(Action::Trace(t)) => {
                // Traces do not reset last_consumed_nav_key as they can wrap actions
                out.push(RenderCommand::Action(Action::Trace(t)));
            }
            RenderCommand::Action(Action::ToggleFocus) => {
                last_consumed_nav_key = None;
                if preview_fullscreen {
                    sim_focus = Focus::Input;
                    out.push(RenderCommand::Action(Action::CyclePreview));
                    continue;
                }
                if sim_focus == Focus::Results {
                    if let Some(actions) = get_nav_bind(focus_binds, "esc") {
                        for action in actions.iter().cloned() {
                            if let Some(action) = action_from_null::<A>(action) {
                                update_sim_focus(&action, &mut sim_focus);
                                out.push(RenderCommand::Action(action));
                            }
                        }
                    } else {
                        out.push(RenderCommand::Action(Action::Quit(130)));
                    }
                } else {
                    sim_focus = match sim_focus {
                        Focus::Input => Focus::Results,
                        Focus::Results => Focus::Input,
                    };
                    out.push(RenderCommand::Action(Action::ToggleFocus));
                }
            }
            RenderCommand::Action(Action::FocusFilter) => {
                last_consumed_nav_key = None;
                sim_focus = Focus::Input;
                out.push(RenderCommand::Action(Action::FocusFilter));
            }
            RenderCommand::Action(Action::FocusNav) => {
                last_consumed_nav_key = None;
                sim_focus = Focus::Results;
                out.push(RenderCommand::Action(Action::FocusNav));
            }
            RenderCommand::Action(Action::ChDir(payload)) => {
                last_consumed_nav_key = None;
                sim_focus = Focus::Results;
                out.push(RenderCommand::Action(Action::ChDir(payload)));
            }
            RenderCommand::Action(Action::Char(c)) if sim_focus == Focus::Results || preview_fullscreen => {
                last_consumed_nav_key = None;
                let key = c.to_string();
                process_results_nav_key(
                    &key,
                    Some(Action::Char(c)),
                    focus_binds,
                    pending_nav_key,
                    sort_menu_active,
                    &mut sim_focus,
                    &mut out,
                    preview_fullscreen,
                );
            }
            RenderCommand::KeyAction { key, action } if sim_focus == Focus::Results || preview_fullscreen => {
                if !preview_fullscreen
                    && last_consumed_nav_key
                        .as_deref()
                        .is_some_and(|k| k.eq_ignore_ascii_case(&key))
                {
                    continue;
                }
                process_results_nav_key(
                    &key,
                    Some(action),
                    focus_binds,
                    pending_nav_key,
                    sort_menu_active,
                    &mut sim_focus,
                    &mut out,
                    preview_fullscreen,
                );
                if !preview_fullscreen && get_nav_bind(focus_binds, &key).is_some() {
                    last_consumed_nav_key = Some(key);
                } else {
                    last_consumed_nav_key = None;
                }
            }
            RenderCommand::KeyAction { key: _, action } => {
                // sim_focus == Focus::Input
                last_consumed_nav_key = None;
                update_sim_focus(&action, &mut sim_focus);
                out.push(RenderCommand::Action(action));
            }
            RenderCommand::Action(Action::DeleteChar) if sim_focus == Focus::Results || preview_fullscreen => {
                last_consumed_nav_key = None;
                process_results_nav_key(
                    "backspace",
                    Some(Action::DeleteChar),
                    focus_binds,
                    pending_nav_key,
                    sort_menu_active,
                    &mut sim_focus,
                    &mut out,
                    preview_fullscreen,
                );
            }
            RenderCommand::Action(Action::BackwardChar) if sim_focus == Focus::Results || preview_fullscreen => {
                last_consumed_nav_key = None;
                process_results_nav_key(
                    "left",
                    Some(Action::BackwardChar),
                    focus_binds,
                    pending_nav_key,
                    sort_menu_active,
                    &mut sim_focus,
                    &mut out,
                    preview_fullscreen,
                );
            }
            RenderCommand::Action(Action::ForwardChar) if sim_focus == Focus::Results || preview_fullscreen => {
                last_consumed_nav_key = None;
                process_results_nav_key(
                    "right",
                    Some(Action::ForwardChar),
                    focus_binds,
                    pending_nav_key,
                    sort_menu_active,
                    &mut sim_focus,
                    &mut out,
                    preview_fullscreen,
                );
            }
            other => {
                last_consumed_nav_key = None;
                out.push(other);
            }
        }
    }

    *buffer = out;
}

fn apply_aliases<T: SSS, S: Selection, A: ActionExt>(
    buffer: &mut Vec<RenderCommand<A>>,
    aliaser: &mut ActionAliaser<T, S, A>,
    dispatcher: &mut MMState<'_, '_, T, S>,
) {
    let mut out = Vec::new();

    for cmd in buffer.drain(..) {
        match cmd {
            RenderCommand::Action(a) => out.extend(
                aliaser(a, dispatcher)
                    .into_iter()
                    .map(RenderCommand::Action),
            ),
            RenderCommand::KeyAction { key, action } => {
                let transformed = aliaser(action, dispatcher);
                for a in transformed {
                    out.push(RenderCommand::KeyAction {
                        key: key.clone(),
                        action: a,
                    });
                }
            }
            other => out.push(other),
        }
    }

    *buffer = out;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn render_loop<'a, W: Write, T: SSS, S: Selection, A: ActionExt>(
    mut ui: UI,
    mut picker_ui: PickerUI<'a, T, S>,
    mut footer_ui: DisplayUI,
    mut preview_ui: Option<PreviewUI>,
    mut tui: Tui<W>,

    mut overlay_ui: Option<OverlayUI<A>>,
    exit_config: ExitConfig,

    mut render_rx: mpsc::UnboundedReceiver<RenderCommand<A>>,
    controller_tx: EventSender,
    bind_tx: BindSender<A>,

    mut dynamic_handlers: DynamicHandlers<T, S>,
    mut ext_handler: Option<ActionExtHandler<T, S, A>>,
    mut ext_aliaser: Option<ActionAliaser<T, S, A>>,
    initializer: Option<Initializer<T, S>>,
    #[cfg(feature = "bracketed-paste")] //
    mut paste_handler: Option<PasteHandler<T, S>>,
) -> Result<Vec<S>, MatchError> {
    let mut state = State::new();
    if ui.config.nav_mode {
        match ui.config.nav_focus_on_start {
            crate::config::NavFocus::Picker => state.focus = Focus::Results,
            crate::config::NavFocus::Filter => state.focus = Focus::Input,
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let sort_order = ui.config.resolve_sort_for_dir(&cwd);
        picker_ui.worker.set_sort_order(sort_order);
    }

    if let Some(handler) = initializer {
        handler(&mut state.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        ));
    }

    let mut click = Click::None;
    // Tracks the last known mouse position for gap hover highlighting.
    let mut mouse_hover: Option<Position> = None;

    // place the initial command in the state where the preview listener can access
    if let Some(ref p) = preview_ui {
        state.update_preview_payload(p.get_initial_command());
    }

    let mut buffer = Vec::with_capacity(256);

    while render_rx.recv_many(&mut buffer, 256).await > 0 {
        while let Ok(cmd) = render_rx.try_recv() {
            buffer.push(cmd);
        }
        if state.iterations == 0 {
            log::debug!("Render loop started");
            state.needs_redraw = true;
        }
        let has_non_tick = buffer.iter().any(|cmd| !matches!(cmd, RenderCommand::Tick));
        if has_non_tick {
            state.needs_redraw = true;
        }
        if let Some(p) = preview_ui.as_mut() {
            if p.is_pan_settling() {
                state.needs_redraw = true;
            }
        }
        let (mut did_pause, mut did_reload, mut did_exit, mut did_resize, mut did_cursor_wrap) =
            (false, false, None, false, false);

        if let Some(aliaser) = &mut ext_aliaser {
            apply_aliases(
                &mut buffer,
                aliaser,
                &mut state.dispatcher(
                    &mut ui,
                    &mut picker_ui,
                    &mut footer_ui,
                    &mut preview_ui,
                    &controller_tx,
                ),
            )
        };

        if state.sort_menu_active {
            apply_sort_menu(&mut buffer, &mut state.sort_menu_active);
        } else if ui.config.nav_mode {
            if !ui.config.nav_passthrough {
                apply_focus_binds(
                    &mut buffer,
                    state.focus,
                    &ui.config.nav_binds,
                    picker_ui.action_visible
                        || overlay_ui.as_ref().map_or(false, |o| o.index().is_some()),
                    &mut state.pending_nav_key,
                    &mut state.sort_menu_active,
                    state.preview_fullscreen,
                );
            }

            if let Some(aliaser) = &mut ext_aliaser {
                apply_aliases(
                    &mut buffer,
                    aliaser,
                    &mut state.dispatcher(
                        &mut ui,
                        &mut picker_ui,
                        &mut footer_ui,
                        &mut preview_ui,
                        &controller_tx,
                    ),
                )
            };
        } else {
            for cmd in &mut buffer {
                if let RenderCommand::KeyAction { action, .. } = cmd {
                    *cmd = RenderCommand::Action(action.clone());
                }
            }
        }

        if state.should_quit {
            log::debug!("Exiting due to should_quit");
            return if picker_ui.selector.is_disabled()
                && let Some((_, item)) = get_current(&picker_ui)
            {
                Ok(vec![item])
            } else {
                Ok(picker_ui.selector.output().collect())
            };
        } else if state.should_quit_nomatch {
            log::debug!("Exiting due to should_quit_nomatch");
            return Err(MatchError::NoMatch);
        }

        let mut events = buffer.drain(..);
        while let Some(event) = events.next() {
            state.clear_interrupt();

            if !matches!(event, RenderCommand::Tick) {
                info!("Received {event:?}");
            } else {
                // log::trace!("Recieved {event:?}");
            }

            match event {
                #[cfg(feature = "bracketed-paste")]
                RenderCommand::Paste(content) => {
                    if let Some(handler) = &mut paste_handler {
                        let content = {
                            handler(
                                content,
                                &state.dispatcher(
                                    &mut ui,
                                    &mut picker_ui,
                                    &mut footer_ui,
                                    &mut preview_ui,
                                    &controller_tx,
                                ),
                            )
                        };
                        if !content.is_empty() {
                            if let Some(x) = overlay_ui.as_mut()
                                && x.index().is_some()
                            {
                                for c in content.chars() {
                                    x.handle_input(c);
                                }
                            } else {
                                picker_ui.query.push_str(&content);
                            }
                        }
                    }
                }
                RenderCommand::Resize(area) => {
                    tui.resize(area);
                    ui.update_dimensions(area);
                }
                RenderCommand::Refresh => {
                    picker_ui.header.init();
                    footer_ui.init();
                    picker_ui.query.set_prompt(None);
                    picker_ui.results.set_status_line(None);
                    tui.redraw();
                }
                RenderCommand::HeaderTable(columns) => {
                    picker_ui.header.header_table(columns);
                }
                RenderCommand::Mouse(mouse) => {
                    use crate::config::Side;
                    // we could also impl this in the aliasing step
                    let pos = Position::from((mouse.column, mouse.row));
                    let layout = state.layout;

                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(p) = preview_ui.as_mut()
                                && p.visible()
                            {
                                let gap_rect = layout.gap;
                                let is_in_drag_area = if !gap_rect.is_empty() {
                                    // Dedicated gap area: click anywhere in the gap to start drag.
                                    gap_rect.contains(pos)
                                } else {
                                    // Fallback: use the legacy drag_width border-edge approach.
                                    let drag_width = p.drag_width();
                                    if drag_width > 0 {
                                        let side = p
                                            .setting()
                                            .map(|s| &s.layout.side)
                                            .unwrap_or(&Side::Right);
                                        match side {
                                            Side::Right => {
                                                let drag_area = Rect {
                                                    x: layout.preview.x,
                                                    y: layout.preview.y,
                                                    width: drag_width,
                                                    height: layout.preview.height,
                                                };
                                                drag_area.contains(pos)
                                            }
                                            Side::Left => {
                                                let drag_area = Rect {
                                                    x: layout.preview.x
                                                        + layout
                                                            .preview
                                                            .width
                                                            .saturating_sub(drag_width),
                                                    y: layout.preview.y,
                                                    width: drag_width,
                                                    height: layout.preview.height,
                                                };
                                                drag_area.contains(pos)
                                            }
                                            Side::Bottom => {
                                                let drag_area = Rect {
                                                    x: layout.preview.x,
                                                    y: layout.preview.y,
                                                    width: layout.preview.width,
                                                    height: drag_width,
                                                };
                                                drag_area.contains(pos)
                                            }
                                            Side::Top => {
                                                let drag_area = Rect {
                                                    x: layout.preview.x,
                                                    y: layout.preview.y
                                                        + layout
                                                            .preview
                                                            .height
                                                            .saturating_sub(drag_width),
                                                    width: layout.preview.width,
                                                    height: drag_width,
                                                };
                                                drag_area.contains(pos)
                                            }
                                        }
                                    } else {
                                        false
                                    }
                                };

                                if is_in_drag_area {
                                    state.dragging = Some(pos);
                                    continue;
                                }
                            }

                            if layout.results.contains(pos) {
                                let y = mouse.row - layout.results.top();
                                debug!("Results clicked at: {y}");
                                click = Click::ResultPos(y);
                            } else if layout.input.contains(pos) {
                                // The X offset of the start of the visible text relative to the terminal
                                let text_start_x = layout.input.x + picker_ui.query.left();

                                if pos.x >= text_start_x {
                                    let visual_offset = pos.x - text_start_x;
                                    picker_ui.query.set_at_visual_offset(visual_offset);
                                } else {
                                    picker_ui.query.set(None, 0);
                                }
                            } else if layout.status.contains(pos) {
                                let x = pos.x.saturating_sub(layout.status.x);
                                debug!("Status clicked at x: {x}");
                                if let Some(action) = find_interaction(
                                    &picker_ui.results.status_config.interactions,
                                    x,
                                ) {
                                    click = Click::Semantic(action);
                                }
                            } else if layout.header.contains(pos) {
                                let rel_x = pos.x.saturating_sub(layout.header.x);
                                let rel_y = pos.y.saturating_sub(layout.header.y);
                                debug!("Header clicked at x: {rel_x}, y: {rel_y}");

                                if let Some(setting) =
                                    picker_ui.header.config.interactions.get(rel_y as usize)
                                    && let Some(action) = find_interaction(setting, rel_x)
                                {
                                    click = Click::Semantic(action);
                                }
                            } else if layout.footer.contains(pos) {
                                let rel_x = pos.x.saturating_sub(layout.footer.x);
                                let rel_y = pos.y.saturating_sub(layout.footer.y);
                                debug!("Footer clicked at x: {rel_x}, y: {rel_y}");

                                if let Some(setting) =
                                    footer_ui.config.interactions.get(rel_y as usize)
                                    && let Some(action) = find_interaction(setting, rel_x)
                                {
                                    click = Click::Semantic(action);
                                }
                            }
                        }
                        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                            if layout.preview.contains(pos) {
                                if let Some(p) = preview_ui.as_mut() {
                                    if matches!(mouse.kind, MouseEventKind::ScrollDown) {
                                        p.down(1)
                                    } else {
                                        p.up(1)
                                    }
                                }
                            } else {
                                let next = matches!(mouse.kind, MouseEventKind::ScrollDown)
                                    ^ picker_ui.results.reverse();
                                did_cursor_wrap = if next {
                                    picker_ui.results.cursor_next()
                                } else {
                                    picker_ui.results.cursor_prev()
                                };
                            }
                        }
                        MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                            let left = matches!(mouse.kind, MouseEventKind::ScrollLeft);
                            if layout.preview.contains(pos) {
                                if let Some(p) = preview_ui.as_mut() {
                                    p.scroll(true, if left { -1 } else { 1 })
                                }
                            } else {
                                if !left
                                    || picker_ui.results.hscroll > 0
                                    || !picker_ui.query.input.is_empty()
                                {
                                    picker_ui
                                        .results
                                        .current_scroll(if left { -1 } else { 1 }, true);
                                }
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(start_pos) = state.dragging {
                                if let Some(p) = preview_ui.as_mut() {
                                    let side =
                                        p.setting().map(|s| &s.layout.side).unwrap_or(&Side::Right);
                                    match side {
                                        Side::Right => {
                                            if pos.x < start_pos.x {
                                                p.expand(start_pos.x - pos.x);
                                            } else if pos.x > start_pos.x {
                                                p.shrink(pos.x - start_pos.x);
                                            }
                                        }
                                        Side::Left => {
                                            if pos.x > start_pos.x {
                                                p.expand(pos.x - start_pos.x);
                                            } else if pos.x < start_pos.x {
                                                p.shrink(start_pos.x - pos.x);
                                            }
                                        }
                                        Side::Bottom => {
                                            if pos.y < start_pos.y {
                                                p.expand(start_pos.y - pos.y);
                                            } else if pos.y > start_pos.y {
                                                p.shrink(pos.y - start_pos.y);
                                            }
                                        }
                                        Side::Top => {
                                            if pos.y > start_pos.y {
                                                p.expand(pos.y - start_pos.y);
                                            } else if pos.y < start_pos.y {
                                                p.shrink(start_pos.y - pos.y);
                                            }
                                        }
                                    }
                                    state.dragging = Some(pos);
                                }
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            state.dragging = None;
                        }
                        MouseEventKind::Moved => {
                            mouse_hover = Some(pos);
                        }
                        _ => {}
                    }
                }
                RenderCommand::NoMatch => {
                    return Err(MatchError::NoMatch);
                }
                RenderCommand::Empty => {
                    return Ok(vec![]);
                }
                RenderCommand::Action(action) | RenderCommand::KeyAction { action, .. } => {
                    if let Some(x) = overlay_ui.as_mut() {
                        if match action {
                            Action::Char(c) => x.handle_input(c),
                            _ => x.handle_action(&action),
                        } {
                            continue;
                        }
                    }
                    let PickerUI {
                        query,
                        action: action_input,
                        action_visible,
                        results,
                        worker,
                        selector: selections,
                        ..
                    } = &mut picker_ui;
                    match action {
                        Action::Select => {
                            if let Some(item) = worker.get_nth(results.index()) {
                                selections.sel(item);
                            }
                        }
                        Action::Deselect => {
                            if let Some(item) = worker.get_nth(results.index()) {
                                selections.desel(item);
                            }
                        }
                        Action::DeselectUp => {
                            results.cursor_prev();
                            if let Some(item) = worker.get_nth(results.index()) {
                                selections.desel(item);
                            }
                        }
                        Action::Toggle => {
                            if let Some(item) = worker.get_nth(results.index()) {
                                if selections.contains(&item) {
                                    selections.toggle(item);
                                    results.cursor_prev();
                                } else {
                                    selections.toggle(item);
                                    results.cursor_next();
                                }
                            }
                        }
                        Action::ToggleUp => {
                            results.cursor_prev();
                            if let Some(item) = worker.get_nth(results.index()) {
                                selections.toggle(item);
                            }
                        }
                        Action::CycleAll => {
                            selections.cycle_all_bg(worker.raw_results());
                        }
                        Action::ClearSelections => {
                            selections.clear();
                        }
                        Action::Accept => {
                            let ret = if selections.is_empty() {
                                if let Some(item) = get_current(&picker_ui) {
                                    vec![item.1]
                                } else if exit_config.allow_empty {
                                    vec![]
                                } else {
                                    continue;
                                }
                            } else {
                                selections.output().collect::<Vec<S>>()
                            };
                            return Ok(ret);
                        }
                        Action::Quit(code) => {
                            if picker_ui.action_visible {
                                picker_ui.action_visible = false;
                                picker_ui.action.cancel();
                                crate::ACTION_BOX_ACTIVE
                                    .store(false, std::sync::atomic::Ordering::Relaxed);
                            } else if !picker_ui.selector.is_empty() {
                                // First Esc: clear multi-selections.
                                picker_ui.selector.clear();
                            } else if !picker_ui.results.yank_paths.is_empty() {
                                // Second Esc: clear yanked paths.
                                picker_ui.results.yank_paths.clear();
                            } else if !picker_ui.results.cut_paths.is_empty() {
                                // Third Esc: clear cut paths.
                                picker_ui.results.cut_paths.clear();
                            } else {
                                return Err(MatchError::Abort(code));
                            }
                        }

                        // Results
                        Action::ToggleWrap => {
                            results.wrap(!results.is_wrap());
                        }
                        Action::ToggleActionBox => {
                            picker_ui.action_visible = !picker_ui.action_visible;
                            crate::ACTION_BOX_ACTIVE.store(
                                picker_ui.action_visible,
                                std::sync::atomic::Ordering::Relaxed,
                            );
                        }
                        Action::ToggleParentPeek => {
                            ui.config.parent_peek.enabled = !ui.config.parent_peek.enabled;
                        }
                        Action::ToggleFooter => {
                            footer_ui.show = !footer_ui.show;
                        }
                        Action::ToggleHeader => {
                            picker_ui.header.show = !picker_ui.header.show;
                        }
                        Action::Up(x) | Action::Down(x) => {
                            let next = matches!(action, Action::Down(_)) ^ results.reverse();
                            for _ in 0..x.into() {
                                did_cursor_wrap = if next {
                                    results.cursor_next()
                                } else {
                                    results.cursor_prev()
                                };
                            }
                        }
                        Action::Pos(pos) => {
                            let (_, status) =
                                crate::nucleo::Worker::new_snapshot(&mut worker.nucleo);
                            results.status = status;
                            let pos = if pos >= 0 {
                                (pos as u32).min(results.end())
                            } else {
                                results.status.matched_count.saturating_sub((-pos) as u32)
                            };
                            results.cursor_jump(pos);
                        }
                        Action::QueryPos(pos) => {
                            let pos = if pos >= 0 {
                                pos as u16
                            } else {
                                (query.len() as u16).saturating_sub((-pos) as u16)
                            };
                            query.set(None, pos);
                        }
                        Action::HScroll(n) | Action::VScroll(n) => {
                            if let Some(p) = &mut preview_ui
                                && !p.config.wrap
                                && false
                            // track mouse location?
                            {
                                p.scroll(true, n);
                            } else if !matches!(action, Action::HScroll(_))
                                || n >= 0
                                || results.hscroll > 0
                                || !query.input.is_empty()
                            {
                                results.current_scroll(n, matches!(action, Action::HScroll(_)));
                            }
                        }
                        Action::HalfPageDown | Action::HalfPageUp => {
                            let x = (results.height() + 1) / 2;
                            let next = matches!(action, Action::HalfPageDown) ^ results.reverse();
                            for _ in 0..x.into() {
                                did_cursor_wrap = if next {
                                    results.cursor_next()
                                } else {
                                    results.cursor_prev()
                                };
                            }
                        }

                        // Preview Navigation
                        Action::PreviewUp(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if n == 0 {
                                    p.reset_scroll();
                                } else {
                                    p.up(n);
                                }
                            }
                        }
                        Action::PreviewDown(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if n == 0 {
                                    p.scroll_end();
                                } else {
                                    p.down(n);
                                }
                            }
                        }
                        Action::ExpandPreview(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.expand(n)
                            }
                        }
                        Action::ShrinkPreview(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.shrink(n)
                            }
                        }
                        Action::PreviewZoomIn | Action::DiagramZoomIn => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.zoom = (p.zoom * 1.25_f32).min(50.0);
                                p.view
                                    .image_id
                                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                                p.view
                                    .changed
                                    .store(true, std::sync::atomic::Ordering::Release);
                            }
                        }
                        Action::PreviewZoomOut | Action::DiagramZoomOut => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.zoom = (p.zoom / 1.25_f32).max(0.1);
                                p.view
                                    .image_id
                                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                                p.view
                                    .changed
                                    .store(true, std::sync::atomic::Ordering::Release);
                            }
                        }
                        Action::PreviewResetZoom | Action::DiagramResetZoom => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.zoom = 1.0_f32;
                                p.pan_x = 0;
                                p.pan_y = 0;
                                p.view
                                    .image_id
                                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                                p.view
                                    .changed
                                    .store(true, std::sync::atomic::Ordering::Release);
                            }
                        }
                        Action::ToggleDiagram => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.toggle_diagram();
                                state.needs_redraw = true;
                            }
                        }
                        Action::PreviewHalfPageUp | Action::PreviewHalfPageDown => {
                            if let Some(p) = preview_ui.as_mut() {
                                let n = (p.area.height + 1) / 2;

                                if matches!(action, Action::PreviewHalfPageUp) {
                                    p.up(n)
                                } else {
                                    p.down(n)
                                }
                            }
                        }

                        Action::PreviewHScroll(x) | Action::PreviewScroll(x) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.scroll(matches!(action, Action::PreviewHScroll(_)), x);
                            }
                        }
                        Action::PreviewJump => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.jump()
                            }
                        }

                        Action::NextDiagram | Action::PrevDiagram => {
                            if let Some(p) = preview_ui.as_mut() {
                                let offsets = p
                                    .view
                                    .diagram_offsets
                                    .lock()
                                    .ok()
                                    .map(|g| g.clone())
                                    .unwrap_or_default();
                                if !offsets.is_empty() {
                                    let current = p.current_offset();
                                    let (target, next_idx) = if matches!(action, Action::NextDiagram) {
                                        // Find first offset strictly greater than current scroll
                                        let found = offsets.iter().copied().enumerate().find(|(_, o)| *o > current);
                                        if let Some((idx, o)) = found {
                                            (o, idx)
                                        } else {
                                            (*offsets.first().unwrap(), 0)
                                        }
                                    } else {
                                        // Find last offset strictly less than current scroll
                                        let found = offsets.iter().copied().enumerate().rfind(|(_, o)| *o < current);
                                        if let Some((idx, o)) = found {
                                            (o, idx)
                                        } else {
                                            (*offsets.last().unwrap(), offsets.len() - 1)
                                        }
                                    };
                                    p.scroll_to(target);

                                    // Update active diagram image from sources if available
                                    if let Ok(sources) = p.view.diagram_sources.lock() {
                                        if let Some(src) = sources.get(next_idx) {
                                            p.view.current_diagram_idx.store(
                                                next_idx,
                                                std::sync::atomic::Ordering::Release,
                                            );
                                            if let Some(img) =
                                                crate::utils::mermaid::render_mermaid_to_image(
                                                    src, 2.0,
                                                )
                                            {
                                                if let Ok(mut guard) = p.view.image.lock() {
                                                    *guard = Some(img);
                                                    p.view.image_id.fetch_add(
                                                        1,
                                                        std::sync::atomic::Ordering::Release,
                                                    );
                                                    p.view.changed.store(
                                                        true,
                                                        std::sync::atomic::Ordering::Release,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    let src_opt = p.view.diagram_sources.lock().ok().and_then(|sources| {
                                        if sources.is_empty() {
                                            return None;
                                        }
                                        let total = sources.len();
                                        let cur = p.view.current_diagram_idx.load(std::sync::atomic::Ordering::Relaxed);
                                        let next_idx = if matches!(action, Action::NextDiagram) {
                                            (cur + 1) % total
                                        } else {
                                            (cur + total.saturating_sub(1)) % total
                                        };
                                        p.view.current_diagram_idx.store(
                                            next_idx,
                                            std::sync::atomic::Ordering::Release,
                                        );
                                        sources.get(next_idx).cloned()
                                    });
                                    if let Some(src) = src_opt {
                                        if let Some(img) = crate::utils::mermaid::render_mermaid_to_image(&src, 2.0) {
                                            if let Ok(mut guard) = p.view.image.lock() {
                                                *guard = Some(img);
                                            }
                                            p.reset_diagram_pan();
                                            p.view.image_id.fetch_add(
                                                1,
                                                std::sync::atomic::Ordering::Release,
                                            );
                                            p.view.changed.store(
                                                true,
                                                std::sync::atomic::Ordering::Release,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Preview
                        Action::CyclePreview => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.cycle_layout();
                                p.current_dimension = None;
                                p.view.image_id.fetch_add(1, std::sync::atomic::Ordering::Release);
                                p.view.changed.store(true, std::sync::atomic::Ordering::Release);
                                state.insert(crate::message::Event::PreviewChange);
                                if !p.command().is_empty() {
                                    state.update_preview_payload(p.command());
                                }
                                if p.is_fullscreen() {
                                    state.focus = Focus::Results;
                                    state.preview_fullscreen = true;
                                } else {
                                    state.focus = Focus::Input;
                                    state.preview_fullscreen = false;
                                }
                                tui.redraw();
                            }
                        }

                        Action::Preview(context) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if !state.update_preview_payload(context.as_str()) {
                                    p.toggle_show()
                                } else {
                                    p.show(true);
                                }
                            };
                        }
                        Action::Help(context) => {
                            if let Some(p) = preview_ui.as_mut() {
                                // empty payload signifies help
                                if !state.update_preview_set(Err(context.into())) {
                                    state.update_preview_unset()
                                } else {
                                    p.show(true);
                                }
                            };
                        }
                        Action::SetPreview(idx) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if let Some(idx) = idx {
                                    p.set_layout(idx);
                                } else {
                                    state.update_preview_payload(p.command());
                                }
                            }
                        }
                        Action::SwitchPreview(idx) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if let Some(idx) = idx {
                                    if !p.set_layout(idx)
                                        && !state.update_preview_payload(p.command())
                                    {
                                        p.toggle_show();
                                    }
                                } else {
                                    p.toggle_show()
                                }
                            }
                        }
                        Action::TogglePreviewWrap => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.wrap(!p.is_wrap());
                            }
                        }

                        // Programmable
                        Action::Execute(payload) => {
                            state.set_interrupt(Interrupt::Execute, payload);
                        }
                        Action::CopyAsync(ref payload) => {
                            state.set_interrupt(Interrupt::ExecuteAsync, payload.clone());
                            state.discriminant_payload = Some(if tui.config.osc52 { 1 } else { 0 });
                        }
                        Action::Copy(ref payload) => {
                            state.set_interrupt(Interrupt::ExecuteSilent, payload.clone());
                            state.discriminant_payload = Some(if tui.config.osc52 { 3 } else { 2 });
                        }
                        Action::ExecuteAsync(ref payload) | Action::ExecuteThen(ref payload) => {
                            let is_async = matches!(action, Action::ExecuteAsync(_));
                            let payload = payload.clone();

                            let mut remainder = crate::action::Actions::default();
                            for cmd in events.by_ref() {
                                if let RenderCommand::Action(a)
                                | RenderCommand::KeyAction { action: a, .. } = cmd
                                {
                                    remainder.push(a);
                                }
                            }

                            if let Some(id) = state.stash_actions(remainder, bind_tx.clone()) {
                                state.set_interrupt(Interrupt::ExecuteAsync, payload);
                                state.discriminant_payload =
                                    Some(2 * id + (if is_async { 1 } else { 0 }));
                            } else {
                                log::error!("No free slots left: remaining actions dropped");
                            }
                        }
                        Action::ExecuteSilent(payload) => {
                            state.set_interrupt(Interrupt::ExecuteSilent, payload);
                        }
                        Action::Store(payload) => {
                            state.envs.set("MM_STORE", payload);
                        }
                        Action::Become(payload) => {
                            state.set_interrupt(Interrupt::Become, payload);
                        }
                        Action::BecomeSilent(payload) => {
                            state.set_interrupt(Interrupt::BecomeSilent, payload);
                        }
                        Action::Reload(payload) => {
                            state.set_interrupt(Interrupt::Reload, payload);
                        }
                        Action::ChDir(payload) => {
                            if ui.config.nav_mode {
                                state.focus = Focus::Results;
                                state.focus_blink = true;
                                state.focus_tick = 0;
                                let prompt = &ui.config.nav_prompt;
                                if !prompt.is_empty() {
                                    picker_ui
                                        .query
                                        .set_prompt(Some(ratatui::text::Line::raw(prompt.clone())));
                                }
                            }
                            state.set_interrupt(Interrupt::ChDir, payload);
                        }
                        Action::Print(payload) => {
                            state.set_interrupt(Interrupt::Print, payload);
                        }

                        // Columns
                        Action::SwitchColumn(col_name) => {
                            if worker.query.active_column_name(query.str_at_cursor()) != col_name
                                && worker.columns.iter().any(|c| *c.name == col_name)
                            {
                                query.prepare_column_change();
                                query.push_str(&format!("%{} ", col_name));
                            } else {
                                log::warn!("Column {} not found in worker columns", col_name);
                            }
                        }
                        Action::NextColumn | Action::PrevColumn => {
                            let cursor_byte = query.byte_index(query.cursor() as usize);
                            let active_idx = worker.query.active_column_index(cursor_byte);

                            let num_columns = worker.columns.len();
                            let has_multiple_columns = num_columns > 1
                                || (num_columns == 1 && !worker.columns[0].name.is_empty());
                            if has_multiple_columns {
                                query.prepare_column_change();

                                let mut next_idx = match action {
                                    Action::NextColumn => active_idx + 1,
                                    Action::PrevColumn => {
                                        active_idx + num_columns - 1 % num_columns
                                    }
                                    _ => unreachable!(),
                                } % num_columns;

                                loop {
                                    if next_idx < results.hidden_columns.len()
                                        && results.hidden_columns[next_idx]
                                    {
                                        next_idx = match action {
                                            Action::NextColumn => (next_idx + 1) % num_columns,
                                            Action::PrevColumn => {
                                                (next_idx + num_columns - 1) % num_columns
                                            }
                                            _ => unreachable!(),
                                        };
                                    } else {
                                        break;
                                    }
                                }

                                let col_name = &worker.columns[next_idx].name;
                                query.push_str(&format!("%{} ", col_name));
                            }
                        }

                        Action::ToggleColumn(col_name) => {
                            let index = if let Some(name) = col_name {
                                worker.columns.iter().position(|c| *c.name == name)
                            } else {
                                let cursor_byte = query.byte_index(query.cursor() as usize);
                                Some(worker.query.active_column_index(cursor_byte))
                            };

                            if let Some(idx) = index {
                                if idx >= results.hidden_columns.len() {
                                    results.hidden_columns.resize(idx + 1, false);
                                }
                                results.hidden_columns[idx] = !results.hidden_columns[idx];
                            }
                        }

                        Action::ShowColumn(col_name) => {
                            if let Some(name) = col_name {
                                if let Some(idx) =
                                    worker.columns.iter().position(|c| *c.name == name)
                                {
                                    if idx < results.hidden_columns.len() {
                                        results.hidden_columns[idx] = false;
                                    }
                                }
                            } else {
                                for val in results.hidden_columns.iter_mut() {
                                    *val = false;
                                }
                            }
                        }

                        // Edit
                        Action::SetQuery(context) => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.set(context, u16::MAX);
                            } else {
                                query.set(context, u16::MAX);
                            }
                        }
                        Action::ForwardChar => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.forward_char()
                            } else {
                                query.forward_char()
                            }
                        }
                        Action::BackwardChar => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.backward_char()
                            } else {
                                query.backward_char()
                            }
                        }
                        Action::ForwardWord => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.forward_word()
                            } else {
                                query.forward_word()
                            }
                        }
                        Action::BackwardWord => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.backward_word()
                            } else {
                                query.backward_word()
                            }
                        }
                        Action::DeleteChar => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.delete()
                            } else {
                                query.delete()
                            }
                        }
                        Action::DeleteWord => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.delete_word()
                            } else {
                                query.delete_word()
                            }
                        }
                        Action::DeleteNextChar => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.delete_next()
                            } else {
                                query.delete_next()
                            }
                        }
                        Action::DeleteNextWord => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.delete_next_word()
                            } else {
                                query.delete_next_word()
                            }
                        }
                        Action::DeleteLineStart => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.delete_line_start()
                            } else {
                                query.delete_line_start()
                            }
                        }
                        Action::DeleteLineEnd => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                action_input.delete_line_end()
                            } else {
                                query.delete_line_end()
                            }
                        }
                        Action::Cancel => {
                            if *action_visible {
                                *action_visible = false;
                                action_input.cancel();
                                crate::ACTION_BOX_ACTIVE
                                    .store(false, std::sync::atomic::Ordering::Relaxed);
                                tui.redraw();
                            } else {
                                query.cancel()
                            }
                        }

                        // Other
                        Action::Redraw => {
                            tui.redraw();
                        }
                        Action::ToggleFocus => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                *action_visible = false;
                                action_input.cancel();
                                crate::ACTION_BOX_ACTIVE
                                    .store(false, std::sync::atomic::Ordering::Relaxed);
                                tui.redraw();
                                continue;
                            }
                            if ui.config.nav_mode {
                                state.focus = match state.focus {
                                    Focus::Input => Focus::Results,
                                    Focus::Results => Focus::Input,
                                };
                                state.focus_blink = true;
                                state.focus_tick = 0;

                                let prompt = &ui.config.nav_prompt;
                                if !prompt.is_empty() {
                                    let prompt = match state.focus {
                                        Focus::Results => {
                                            Some(ratatui::text::Line::raw(prompt.clone()))
                                        }
                                        Focus::Input => None,
                                    };
                                    picker_ui.query.set_prompt(prompt);
                                }
                            }
                        }
                        Action::FocusFilter => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                *action_visible = false;
                                action_input.cancel();
                                crate::ACTION_BOX_ACTIVE
                                    .store(false, std::sync::atomic::Ordering::Relaxed);
                                tui.redraw();
                                continue;
                            }
                            if ui.config.nav_mode {
                                state.focus = Focus::Input;
                                state.focus_blink = true;
                                state.focus_tick = 0;
                                let prompt = &ui.config.nav_prompt;
                                if !prompt.is_empty() {
                                    picker_ui.query.set_prompt(None);
                                }
                            }
                        }
                        Action::FocusNav => {
                            if *action_visible
                                && crate::ACTION_BOX_ACTIVE
                                    .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                *action_visible = false;
                                action_input.cancel();
                                crate::ACTION_BOX_ACTIVE
                                    .store(false, std::sync::atomic::Ordering::Relaxed);
                                tui.redraw();
                                continue;
                            }
                            if ui.config.nav_mode {
                                state.focus = Focus::Results;
                                state.focus_blink = true;
                                state.focus_tick = 0;
                                let prompt = &ui.config.nav_prompt;
                                if !prompt.is_empty() {
                                    picker_ui
                                        .query
                                        .set_prompt(Some(ratatui::text::Line::raw(prompt.clone())));
                                }
                            }
                        }
                        Action::Overlay(index) => {
                            if let Some(x) = overlay_ui.as_mut() {
                                x.enable(index, &ui.area());
                                tui.redraw();
                            };
                        }
                        Action::Custom(e) => {
                            if let Some(handler) = &mut ext_handler {
                                handler(
                                    e,
                                    &mut state.dispatcher(
                                        &mut ui,
                                        &mut picker_ui,
                                        &mut footer_ui,
                                        &mut preview_ui,
                                        &controller_tx,
                                    ),
                                );
                            }
                        }
                        Action::Char(c) => {
                            if !c.is_ascii_control() {
                                let is_action_box_input = *action_visible
                                    && crate::ACTION_BOX_ACTIVE
                                        .load(std::sync::atomic::Ordering::Relaxed);
                                if is_action_box_input {
                                    action_input.push_char(c)
                                } else {
                                    if *action_visible {
                                        *action_visible = false;
                                        action_input.cancel();
                                        crate::ACTION_BOX_ACTIVE
                                            .store(false, std::sync::atomic::Ordering::Relaxed);
                                    }
                                    if !(ui.config.nav_mode
                                        && !ui.config.nav_passthrough
                                        && state.focus == Focus::Results)
                                    {
                                        query.push_char(c)
                                    }
                                }
                            }
                        }
                        Action::SetMode(s) => {
                            if let Ok(mut m) = crate::MODE.lock() {
                                *m = s;
                            }
                        }
                        Action::Sort(order) => {
                            picker_ui.worker.set_sort_order(order);
                            state.sort_menu_active = false;
                            state.needs_redraw = true;
                        }
                        Action::SortMenu => {
                            state.sort_menu_active = !state.sort_menu_active;
                            state.needs_redraw = true;
                        }

                        // unreachable
                        Action::PrintKey => {}
                        Action::Semantic(_) => {}
                        Action::Trace(_) => {}
                    }
                }
                _ => {}
            }

            let interrupt = state.interrupt();

            match interrupt {
                Interrupt::None => continue,
                Interrupt::Execute => {
                    // because of this, we don't want to send controller events until after resuming at batch end
                    if controller_tx.send(Event::Pause).is_err() {
                        break;
                    }
                    tui.enter_execute();
                    if did_exit.is_none() {
                        did_exit = Some(true);
                    }
                    did_pause = true;
                }
                Interrupt::Reload => {
                    state.synced = [false; 2];
                    did_reload = true;
                }
                Interrupt::ChDir => {
                    state.synced = [false; 2];
                }
                Interrupt::Become => {
                    tui.exit(None);
                }
                Interrupt::BecomeSilent => {
                    tui.exit_lite();
                }
                _ => {}
            }
            // Apply interrupt effect
            {
                let mut dispatcher = state.dispatcher(
                    &mut ui,
                    &mut picker_ui,
                    &mut footer_ui,
                    &mut preview_ui,
                    &controller_tx,
                );
                for h in dynamic_handlers.1.get_mut(interrupt) {
                    h(&mut dispatcher);
                }

                if matches!(interrupt, Interrupt::ChDir) {
                    if let Ok(cwd) = std::env::current_dir() {
                        let sort_order = ui.config.resolve_sort_for_dir(&cwd);
                        picker_ui.worker.set_sort_order(sort_order);
                    }
                }

                if matches!(interrupt, Interrupt::Become) {
                    return Err(MatchError::Become(state.payload().clone()));
                }
            }

            if state.should_quit {
                log::debug!("Exiting due to should_quit");
                return if picker_ui.selector.is_disabled()
                    && let Some((_, item)) = get_current(&picker_ui)
                {
                    Ok(vec![item])
                } else {
                    Ok(picker_ui.selector.output().collect())
                };
            } else if state.should_quit_nomatch {
                log::debug!("Exiting due to should_quit_nomatch");
                return Err(MatchError::NoMatch);
            }
        }

        // debug!("{state:?}");

        // ------------- update state + render ------------------------
        if state.filtering || did_reload || state.reloading {
            picker_ui.update();
        }
        if did_cursor_wrap {
            log::trace!("cursor wrapped"); // todo: event handler?
        }

        // process exit conditions
        if exit_config.select_1
            && picker_ui.results.status.matched_count == 1
            && let Some((_, item)) = get_current(&picker_ui)
        {
            return Ok(vec![item]);
        }

        // resume tui
        if let Some(clear) = did_exit {
            tui.return_execute(clear)
                .map_err(|e| MatchError::TUIError(e.to_string()))?;
            tui.redraw();
            state.needs_redraw = true;
        }

        if did_reload || did_resize {
            state.needs_redraw = true;
        }

        if picker_ui
            .worker
            .matcher_dirty
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            state.needs_redraw = true;
        }

        if picker_ui.results.status.changed || picker_ui.results.status.running {
            state.needs_redraw = true;
        }

        if preview_ui.as_ref().map_or(false, |p| p.view.changed()) {
            state.needs_redraw = true;
        }

        if !picker_ui.results.config.spinner_prefix.is_empty() {
            state.needs_redraw = true;
        }

        if !picker_ui.results.flash_targets.is_empty() {
            let now = std::time::Instant::now();
            let prev_len = picker_ui.results.flash_targets.len();
            picker_ui.results.flash_targets.retain(|_, (start, _)| {
                now.duration_since(*start) < std::time::Duration::from_millis(1500)
            });
            if !picker_ui.results.flash_targets.is_empty()
                || prev_len != picker_ui.results.flash_targets.len()
            {
                state.needs_redraw = true;
            }
        }

        if ui.config.nav_mode {
            let blink_ticks = ui.config.nav_blink_rate.ticks();
            let prev_blink = state.focus_blink;
            state.focus_tick = state.focus_tick.wrapping_add(1);
            if state.focus_tick >= blink_ticks {
                state.focus_tick = 0;
                state.focus_blink = !state.focus_blink;
            }
            if prev_blink != state.focus_blink {
                state.needs_redraw = true;
            }
        }

        if state.needs_redraw {
            let mut overlay_ui_ref = overlay_ui.as_mut();
            let mut cursor_y_offset = 0;

            tui.terminal
                .draw(|frame| {
                    let mut area = frame.area();

                    // mutates area!
                    render_ui(frame, &mut area, &ui);

                    let mut _area = area;

                    let full_width_footer = footer_ui.is_single_column()
                        && footer_ui.config.row_connection == RowConnectionStyle::Full;

                    let mut breadcrumb_spans = Vec::new();
                    let mut is_global_breadcrumb = false;
                    let mut global_breadcrumb_rect = Rect::default();

                    if picker_ui.breadcrumb_config.show {
                        if let Ok(cwd) = std::env::current_dir() {
                            let mut components = Vec::new();
                            let home_dir = std::env::var("HOME").ok().map(std::path::PathBuf::from);

                            if let Some(home) = home_dir {
                                if let Ok(stripped) = cwd.strip_prefix(&home) {
                                    components.push("~".to_string());
                                    for comp in stripped.components() {
                                        components
                                            .push(comp.as_os_str().to_string_lossy().to_string());
                                    }
                                }
                            }

                            if components.is_empty() {
                                for comp in cwd.components() {
                                    components.push(comp.as_os_str().to_string_lossy().to_string());
                                }
                            }

                            if picker_ui.breadcrumb_config.current_folder_only
                                && !components.is_empty()
                            {
                                let last = components.pop().unwrap();
                                components.clear();
                                components.push(last);
                            }

                            let truncate_len = picker_ui.breadcrumb_config.truncate_length;
                            let num_components = components.len();
                            let mut breadcrumb_width = 0;

                            for (i, mut text) in components.into_iter().enumerate() {
                                if truncate_len > 0 && i < num_components.saturating_sub(1) {
                                    if text != "~" && text.chars().count() > truncate_len {
                                        text = text.chars().take(truncate_len).collect();
                                    }
                                }

                                breadcrumb_width += text.chars().count() as u16;
                                breadcrumb_spans.push(ratatui::text::Span::styled(
                                    text,
                                    ratatui::style::Style::from(
                                        picker_ui.breadcrumb_config.style.clone(),
                                    ),
                                ));
                                if i < num_components - 1 {
                                    let sep = picker_ui.breadcrumb_config.separator.clone();
                                    breadcrumb_width += sep.chars().count() as u16;
                                    breadcrumb_spans.push(ratatui::text::Span::styled(
                                        sep,
                                        ratatui::style::Style::from(
                                            picker_ui.breadcrumb_config.separator_style.clone(),
                                        ),
                                    ));
                                }
                            }

                            // Determine if we need global breadcrumb
                            let has_preview = preview_ui.as_ref().is_some_and(|p| p.visible());
                            let picker_w = if has_preview {
                                let [_, p_area, _] = preview_ui.as_ref().unwrap().split(_area);
                                p_area.width
                            } else {
                                _area.width
                            };

                            if has_preview || breadcrumb_width > picker_w {
                                is_global_breadcrumb = true;
                                global_breadcrumb_rect = split(&mut _area, 1, !picker_ui.reverse());
                            }
                        }
                    }

                    let show_sort_menu = state.sort_menu_active;
                    let show_nav_hints = if state.preview_fullscreen {
                        true
                    } else {
                        ui.config.nav_mode
                            && ui.config.nav_hints
                            && state.focus == Focus::Results
                            && !footer_ui.show
                    };

                    let effective_footer_height = if state.preview_fullscreen {
                        ui.config.nav_hints_height(PREVIEW_NAV_HINTS.len()).max(1)
                    } else if show_sort_menu {
                        ui.config.sort_menu.height(SORT_MENU_ITEMS.len())
                    } else if footer_ui.show {
                        footer_ui.height()
                    } else if show_nav_hints {
                        let count = if ui.config.nav_basic {
                            BASIC_NAV_HINTS.len()
                        } else {
                            NAV_HINTS.len()
                        };
                        ui.config.nav_hints_height(count)
                    } else {
                        0
                    };

                    let is_full_footer = full_width_footer || show_nav_hints || show_sort_menu;

                    let mut footer =
                        if is_full_footer || preview_ui.as_ref().is_none_or(|p| !p.visible()) {
                            split(&mut _area, effective_footer_height, picker_ui.reverse())
                        } else {
                            Rect::default()
                        };

                    // Compute how wide the gap needs to be to show the counter.
                    // Rules:
                    //   - suppress selected if sel_count == yank_count (fully covered by yank)
                    //   - suppress yank    if yank_count == cut_count  (fully covered by cut)
                    //   - if only one group: horizontal, width = " N " of that group
                    //   - if multiple groups: vertical (one row each), width = widest " N "
                    let _counter_gap_width: u16 = {
                        let sel_raw = picker_ui.selector.len();
                        let yank_raw = picker_ui.results.yank_paths.len();
                        let cut_raw = picker_ui.results.cut_paths.len();

                        let show_cut = cut_raw > 0;
                        let show_yank = yank_raw > 0 && yank_raw != cut_raw;
                        let show_sel = sel_raw > 0 && sel_raw != yank_raw && sel_raw != cut_raw;

                        let widths: Vec<usize> = [
                            show_cut.then(|| cut_raw.to_string().len() + 2),
                            show_yank.then(|| yank_raw.to_string().len() + 2),
                            show_sel.then(|| sel_raw.to_string().len() + 2),
                        ]
                        .into_iter()
                        .flatten()
                        .collect();

                        widths.iter().copied().max().unwrap_or(0) as u16
                    };

                    let mut parent_peek_rect = Rect::default();
                    if ui.config.parent_peek.enabled && _area.width >= 50 {
                        let top_offset = if !picker_ui.reverse() && picker_ui.query.config.show {
                            1 + picker_ui.query.config.border.height()
                        } else {
                            0
                        };
                        let pw = ui
                            .config
                            .parent_peek
                            .pct
                            .compute_clamped(_area.width, 10, 30);
                        parent_peek_rect = Rect {
                            x: _area.x,
                            y: _area.y + top_offset,
                            width: pw,
                            height: _area.height.saturating_sub(top_offset),
                        };
                        _area.x += pw;
                        _area.width -= pw;
                    }

                    let [preview, picker_area, footer, gap_area] = if let Some(preview_ui) =
                        preview_ui.as_mut()
                        && preview_ui.visible()
                    {
                        if state.preview_fullscreen {
                            let (preview, footer) = if footer.height > 0 {
                                (_area, footer)
                            } else {
                                let mut full_area = _area;
                                let f = split(
                                    &mut full_area,
                                    effective_footer_height,
                                    false,
                                );
                                (full_area, f)
                            };
                            [preview, Rect::default(), footer, Rect::default()]
                        } else {
                            // Temporarily widen the gap so the counter fits horizontally.
                            let original_gap = preview_ui.setting().map(|s| s.layout.gap).unwrap_or(0);
                            let effective_gap = original_gap.max(if _counter_gap_width > 0 {
                                _counter_gap_width + 2
                            } else {
                                0
                            });
                            if let Some(s) = preview_ui.setting_mut() {
                                s.layout.gap = effective_gap;
                            }
                            let [preview, mut picker_area, gap_area] = preview_ui.split(_area);
                            // Restore the configured gap so nothing else is affected.
                            if let Some(s) = preview_ui.setting_mut() {
                                s.layout.gap = original_gap;
                            }

                            if state.iterations == 0 && _area.width < 30 && picker_area.width <= 5 {
                                warn!("UI too narrow, hiding preview");
                                preview_ui.show(false);

                                [Rect::default(), _area, footer, Rect::default()]
                            } else {
                                if !is_full_footer {
                                    footer = split(
                                        &mut picker_area,
                                        effective_footer_height,
                                        picker_ui.reverse(),
                                    );
                                }

                                [preview, picker_area, footer, gap_area]
                            }
                        }
                    } else {
                        [Rect::default(), _area, footer, Rect::default()]
                    };

                    let original_breadcrumb_show = picker_ui.breadcrumb_config.show;
                    if is_global_breadcrumb {
                        picker_ui.breadcrumb_config.show = false;
                    }
                    let [breadcrumb, action, input, status, header, results] =
                        picker_ui.layout(picker_area);
                    if is_global_breadcrumb {
                        picker_ui.breadcrumb_config.show = original_breadcrumb_show;
                    }

                    let mut footer = footer;
                    if !picker_ui.reverse() && footer.height > 0 {
                        let picker_bottom = results.y + results.height;
                        let preview_bottom = if preview.height > 0 {
                            preview.y + preview.height
                        } else {
                            0
                        };
                        let content_bottom = picker_bottom.max(preview_bottom);
                        if content_bottom < footer.y {
                            footer.y = content_bottom;
                        }
                    }

                    if parent_peek_rect.width > 0 {
                        if footer.height > 0 {
                            let available_h = footer.y.saturating_sub(parent_peek_rect.y);
                            parent_peek_rect.height = parent_peek_rect.height.min(available_h);
                        }
                        if let Some(max_h) = ui.config.parent_peek.max {
                            parent_peek_rect.height = parent_peek_rect.height.min(max_h);
                        }
                    }

                    // save dimensions and check if dimensions changed
                    did_resize = state.update_layout(Layout {
                        preview,
                        action,
                        input,
                        status,
                        header,
                        results,
                        footer,
                        gap: gap_area,
                        pane: _area,
                    });

                    if did_resize {
                        picker_ui.results.update_dimensions(&results);
                        picker_ui.action.update_width(action.width);
                        picker_ui.query.update_width(input.width);
                        footer_ui.update_width(
                            if footer_ui.config.row_connection == RowConnectionStyle::Capped {
                                area.width
                            } else {
                                footer.width
                            },
                        );
                        picker_ui.header.update_width(header.width);
                        // although these only want update when the whole ui change
                        ui.update_dimensions(area);
                        if let Some(x) = overlay_ui_ref.as_deref_mut() {
                            x.update_dimensions(&area);
                        }
                        if let Some(preview_ui) = preview_ui.as_mut() {
                            preview_ui.update_dimensions(&preview);
                        }
                    };

                    let status_inline_label: Option<Line<'_>> =
                        if picker_ui.query.config.status_inline {
                            Some(picker_ui.results.status_line())
                        } else {
                            None
                        };

                    let nav_color = ui.config.nav_color;
                    let nav_mode = ui.config.nav_mode;
                    let nav_do_blink = ui.config.nav_blink;
                    let nav_bold = ui.config.nav_bold;
                    let nav_bar = ui.config.nav_bar;
                    let nav_marker = ui.config.nav_marker.clone();
                    let nav_char = picker_ui
                        .results
                        .config
                        .multi_prefix
                        .chars()
                        .next()
                        .unwrap_or('│')
                        .to_string();
                    let input_focus_info = nav_mode.then_some(FocusInfo {
                        focused: state.focus == Focus::Input,
                        blink_phase: state.focus_blink,
                        color: nav_color,
                        do_blink: nav_do_blink,
                        bold: nav_bold,
                        bar: None,
                        marker: String::new(),
                        nav_char: nav_char.clone(),
                        nav_prompt: ui.config.nav_prompt.clone(),
                    });
                    let results_focus_info = nav_mode.then_some(FocusInfo {
                        focused: state.focus == Focus::Results,
                        blink_phase: state.focus_blink,
                        color: nav_color,
                        do_blink: nav_do_blink,
                        bold: nav_bold,
                        bar: nav_bar,
                        marker: nav_marker,
                        nav_char,
                        nav_prompt: ui.config.nav_prompt.clone(),
                    });

                    if picker_ui.action_visible {
                        let cfg = &picker_ui.action_config;
                        // Apply width percentage — centered within the allocated row(s).
                        let action_w = cfg.width_pct.compute_clamped(action.width, 1, 0);
                        let x_pad = action.width.saturating_sub(action_w) / 2;
                        let action_full_rect = Rect {
                            x: action.x + x_pad,
                            y: action.y,
                            width: action_w,
                            height: action.height,
                        };
                        // Render the separator border (default: bottom line) over the full area.
                        // It only draws at the edges of `action_full_rect`, so the input and
                        // preview content rendered afterwards is not obscured.
                        if !cfg.border.is_empty() {
                            frame.render_widget(cfg.border.as_static_block(), action_full_rect);
                        }

                        let mut current_y = action_full_rect.y;

                        let action_input_rect = Rect {
                            x: action_full_rect.x,
                            y: current_y,
                            width: action_w,
                            height: action_full_rect.height.min(1),
                        };
                        render_input(frame, action_input_rect, &mut picker_ui.action, None, None);
                        current_y += 1;

                        // Render preview area below if preview_height > 0.
                        if cfg.preview_height > 0
                            && action_full_rect.height > current_y - action_full_rect.y
                        {
                            let preview_rect = Rect {
                                x: action_full_rect.x,
                                y: current_y,
                                width: action_w,
                                height: cfg.preview_height,
                            };
                            let block = ratatui::widgets::Block::bordered();
                            frame.render_widget(block, preview_rect);
                        }
                    }

                    if !state.preview_fullscreen {
                        if picker_ui.breadcrumb_config.show && !breadcrumb_spans.is_empty() {
                            let target_rect = if is_global_breadcrumb {
                                global_breadcrumb_rect
                            } else {
                                breadcrumb
                            };

                            if target_rect.height > 0 {
                                let p = ratatui::widgets::Paragraph::new(ratatui::text::Line::from(
                                    breadcrumb_spans,
                                ));
                                frame.render_widget(p, target_rect);
                            }
                        }
                        if picker_ui.query.config.show {
                            cursor_y_offset = render_input(
                                frame,
                                input,
                                &mut picker_ui.query,
                                status_inline_label,
                                input_focus_info,
                            )
                            .y;
                        } else {
                            cursor_y_offset = input.y;
                        }
                        // When status_inline is active, skip the separate status row.
                        if !picker_ui.query.config.status_inline {
                            render_status(frame, status, &picker_ui.results, ui.area().width);
                        }
                        render_results(
                            frame,
                            results,
                            &mut picker_ui,
                            &mut click,
                            results_focus_info,
                            state.reloading,
                        );
                        render_display(frame, header, &mut picker_ui.header, &picker_ui.results);
                        if parent_peek_rect.width > 0 {
                            render_parent_peek(frame, parent_peek_rect, &ui.config.parent_peek);
                        }
                    }

                    if state.preview_fullscreen {
                        if footer.height > 0 {
                            let zoom_pct = preview_ui.as_ref().map(|p| (p.zoom * 100.0).round() as u32);
                            render_nav_hints(
                                frame,
                                footer,
                                false,
                                ui.config.nav_hints_columns,
                                true,
                                zoom_pct,
                            );
                        }
                    } else if show_sort_menu && footer.height > 0 {
                        render_sort_menu(frame, footer, &ui.config.sort_menu);
                    } else {
                        render_display(frame, footer, &mut footer_ui, &picker_ui.results);
                        if show_nav_hints && footer.height > 0 {
                            render_nav_hints(
                                frame,
                                footer,
                                ui.config.nav_basic,
                                ui.config.nav_hints_columns,
                                false,
                                None,
                            );
                        }
                    }
                    if let Some(preview_ui) = preview_ui.as_mut() {
                        state.update_preview_visible(preview_ui);
                        if preview_ui.visible() {
                            // Set the dynamic title from the first column of the current item.
                            let item_title = picker_ui
                                .worker
                                .get_nth(picker_ui.results.index())
                                .map(|item| picker_ui.worker.columns[0].raw(item).into_owned());
                            preview_ui.set_title(item_title);
                            render_preview(frame, preview, preview_ui);

                            // Highlight the gap area when the mouse is hovering over it.
                            if !gap_area.is_empty() {
                                let is_hovered = mouse_hover.is_some_and(|p| gap_area.contains(p));
                                let is_dragging = state.dragging.is_some();
                                if is_hovered || is_dragging {
                                    let gap_block = Block::default()
                                        .style(Style::default().bg(Color::DarkGray));
                                    frame.render_widget(gap_block, gap_area);
                                }

                                // Counter bar: render cut / yank / selected counts.
                                // - suppress selected if fully covered by yank (same count)
                                // - suppress yank    if fully covered by cut  (same count)
                                // - single group → one horizontal row
                                // - multiple groups → stacked vertically, one row per group
                                // - positioned 5 rows from the bottom of the gap
                                let sel_raw = picker_ui.selector.len();
                                let yank_raw = picker_ui.results.yank_paths.len();
                                let cut_raw = picker_ui.results.cut_paths.len();

                                let show_cut = cut_raw > 0;
                                let show_yank = yank_raw > 0 && yank_raw != cut_raw;
                                let show_sel =
                                    sel_raw > 0 && sel_raw != yank_raw && sel_raw != cut_raw;

                                // Collect the groups to render (cut first = highest priority).
                                let rcfg = &picker_ui.results.config;
                                let groups: Vec<(usize, Color)> = [
                                    show_cut.then(|| {
                                        (cut_raw, rcfg.cut_prefix_style.fg.unwrap_or(Color::Red))
                                    }),
                                    show_yank.then(|| {
                                        (
                                            yank_raw,
                                            rcfg.yank_prefix_style.fg.unwrap_or(Color::Yellow),
                                        )
                                    }),
                                    show_sel.then(|| {
                                        (
                                            sel_raw,
                                            rcfg.selected_prefix_style.fg.unwrap_or(Color::Cyan),
                                        )
                                    }),
                                ]
                                .into_iter()
                                .flatten()
                                .collect();

                                if !groups.is_empty() {
                                    let n_rows = groups.len() as u16;
                                    // Position from the top of the gap using the configured offset.
                                    let offset_from_top = preview_ui
                                        .setting()
                                        .map(|s| s.layout.gap_counter_offset)
                                        .unwrap_or(0);
                                    let start_y = (gap_area.y + offset_from_top)
                                        .min(gap_area.y + gap_area.height.saturating_sub(n_rows));

                                    for (row_idx, (count, bg)) in groups.iter().enumerate() {
                                        let row_y = start_y + row_idx as u16;
                                        if row_y >= gap_area.y + gap_area.height {
                                            break;
                                        }
                                        let row_rect = Rect {
                                            x: gap_area.x,
                                            y: row_y,
                                            width: gap_area.width,
                                            height: 1,
                                        };
                                        let span = Span::styled(
                                            format!(" {} ", count),
                                            Style::default().fg(Color::Black).bg(*bg),
                                        );
                                        frame.render_widget(
                                            Paragraph::new(Line::from(span))
                                                .alignment(ratatui::layout::Alignment::Center),
                                            row_rect,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    if let Some(x) = overlay_ui_ref {
                        x.draw(frame);
                    }
                })
                .map_err(|e| MatchError::TUIError(e.to_string()))?;
            state.needs_redraw = false;
        }

        if did_resize {
            // useful to clear artifacts
            if tui.config.redraw_on_resize && did_exit.is_none() {
                tui.redraw();
            }
        }

        drop(events);
        buffer.clear();

        // note: the remainder could be scoped by a conditional on having run?
        // ====== Event handling ==========
        state.update(&picker_ui, &overlay_ui);

        let events = state.events();

        // ---- Invoke handlers -------
        let mut dispatcher = state.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );
        // if let Some((signal, handler)) = signal_handler &&
        // let s = signal.load(std::sync::atomic::Ordering::Acquire) &&
        // s > 0
        // {
        //     handler(s, &mut dispatcher);
        //     signal.store(0, std::sync::atomic::Ordering::Release);
        // };

        // ping handlers with events
        for e in events.iter() {
            for h in dynamic_handlers.0.get(e) {
                h(&mut dispatcher, &e)
            }
        }
        state.reset();

        // ------------------------------
        // send events into controller
        for e in events.iter() {
            controller_tx.send(e)._elog();
        }
        // =================================

        if did_pause {
            log::debug!("Waiting for ack response to pause");
            if controller_tx.send(Event::Resume).is_err() {
                break;
            };
            // due to control flow, this does nothing, but is anyhow a useful safeguard to guarantee the pause
            while let Some(msg) = render_rx.recv().await {
                if matches!(msg, RenderCommand::Ack) {
                    log::debug!("Received ack response to pause");
                    break;
                }
            }
        }
        if did_reload {
            controller_tx.send(Event::Reloaded)._elog();
        }

        click.process(&mut buffer, &bind_tx);
    }

    Err(MatchError::EventLoopClosed)
}

// ------------------------- HELPERS ----------------------------

struct FocusInfo {
    focused: bool,
    blink_phase: bool,
    color: Color,
    do_blink: bool,
    bold: bool,
    bar: Option<ratatui::widgets::BorderType>,
    marker: String,
    nav_char: String,
    nav_prompt: String,
}

impl FocusInfo {
    fn indicator_color(&self) -> Option<Color> {
        if self.focused {
            Some(if self.do_blink && !self.blink_phase {
                Color::DarkGray
            } else {
                self.color
            })
        } else {
            None
        }
    }
}

pub enum Click {
    None,
    ResultPos(u16),
    ResultIdx(u32),
    Semantic(String),
}

impl Click {
    fn process<A: ActionExt>(
        &mut self,
        buffer: &mut Vec<RenderCommand<A>>,
        bind_tx: &BindSender<A>,
    ) {
        match self {
            Self::ResultIdx(u) => {
                buffer.push(RenderCommand::Action(Action::Pos(*u as i32)));
            }
            Self::Semantic(s) => {
                bind_tx
                    .send(BindDirective::Action(Action::Semantic(s.clone())))
                    ._elog();
                log::debug!("Click triggered: @{s}");
            }
            _ => {}
        }
        *self = Click::None
    }
}

fn find_interaction(setting: &crate::config::InteractionRegionSetting, x: u16) -> Option<String> {
    setting
        .iter()
        .rev()
        .find(|(start, _)| x >= *start as u16)
        .map(|(_, action)| action.clone())
        .filter(|a| !a.is_empty())
}

fn render_preview(frame: &mut Frame, area: Rect, ui: &mut PreviewUI) {
    assert!(ui.visible()); // don't call if not visible.
    ui.update_dimensions(&area);

    let has_markdown = ui.has_markdown();
    let has_diagram = ui.has_diagram();
    let is_diagram_image = has_markdown && has_diagram && ui.show_diagram;
    let image_state_ready = ui.get_image_state().is_some();
    let is_image = if has_markdown {
        is_diagram_image && image_state_ready
    } else {
        image_state_ready
    };
    if is_image {
        let block = if ui.is_fullscreen() {
            None
        } else {
            ui.make_block()
        };
        let inner_area = if let Some(b) = &block {
            b.inner(area)
        } else {
            area
        };
        if let Some(b) = block {
            frame.render_widget(b, area);
        }

        let media_fit_str = ui.config.media_fit.as_deref().unwrap_or("fit");
        let resize_mode = match media_fit_str.to_lowercase().as_str() {
            "crop" | "cover" => ratatui_image::Resize::Crop(None),
            "scale" | "stretch" | "fit" | "contain" => ratatui_image::Resize::Scale(None),
            _ => ratatui_image::Resize::Fit(None),
        };

        if let Some(state) = ui.get_image_state() {
            let image_widget = ratatui_image::StatefulImage::new().resize(resize_mode);
            frame.render_stateful_widget(image_widget, inner_area, state);
        }
    } else {
        let widget = ui.make_preview();
        frame.render_widget(widget, area);
    }

    ui.render_scrollbar(frame, area);
}

fn render_results<T: SSS, S: Selection>(
    frame: &mut Frame,
    mut area: Rect,
    ui: &mut PickerUI<T, S>,
    click: &mut Click,
    focus_info: Option<FocusInfo>,
    freeze_snapshot: bool,
) {
    let nav_bar_style = focus_info.as_ref().and_then(|fi| {
        let bar_color = fi.indicator_color();
        if let (Some(border_type), Some(color)) = (fi.bar, bar_color) {
            let mut style = Style::default().fg(color);
            if fi.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            Some((border_type, style))
        } else {
            None
        }
    });

    let cap = matches!(ui.results.config.row_connection, RowConnectionStyle::Capped);
    let (widget, table_width) = ui.make_table(click, nav_bar_style, freeze_snapshot);

    if cap {
        area.width = area.width.min(table_width);
    }

    frame.render_widget(widget, area);

    if let Some(fi) = focus_info {
        let bar_color = fi.indicator_color();

        if !fi.marker.is_empty()
            && let Some(cursor_row) = ui.results.cursor_offset()
        {
            let marker_y = if ui.results.reverse() {
                area.y + area.height.saturating_sub(1 + cursor_row)
            } else {
                area.y + cursor_row
            };
            if marker_y >= area.y && marker_y < area.y + area.height {
                let marker_color = bar_color.unwrap_or(fi.color);
                let marker_w = unicode_width::UnicodeWidthStr::width(fi.marker.as_str()) as u16;
                let marker_rect = Rect {
                    x: area.x,
                    y: marker_y,
                    width: marker_w.min(area.width),
                    height: 1,
                };
                let span = Span::styled(fi.marker.clone(), Style::default().fg(marker_color));
                frame.render_widget(Paragraph::new(span), marker_rect);
            }
        }
    }
}

/// Returns the offset of the cursor against the drawing area
fn render_input(
    frame: &mut Frame,
    area: Rect,
    ui: &mut QueryUI,
    status: Option<Line<'_>>,
    focus_info: Option<FocusInfo>,
) -> Position {
    ui.scroll_to_cursor();
    let focused = focus_info.as_ref().map_or(true, |f| f.focused);
    let nav_prompt = focus_info.as_ref().map(|f| f.nav_prompt.as_str());

    let active_prompt = ui.active_prompt(focused, nav_prompt);
    let widget = if let Some(label) = status {
        ui.make_input_with_status_focused(label, area.width, focused, nav_prompt)
    } else {
        ui.make_input_focused(area.width, focused, nav_prompt)
    };
    let p = ui.cursor_offset_for_prompt(&area, &active_prompt);

    let show_cursor = focused;
    if let CursorSetting::Default = ui.config.cursor {
        if show_cursor {
            frame.set_cursor_position(p)
        }
    };

    frame.render_widget(widget, area);

    if let Some(fi) = focus_info {
        if let (Some(border_type), Some(color)) = (fi.bar, fi.indicator_color()) {
            let mut style = Style::default().fg(color);
            if fi.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if border_type == ratatui::widgets::BorderType::Thick {
                let indicator_rect = Rect {
                    x: area.x,
                    y: area.y,
                    width: 1,
                    height: area.height,
                };
                let span = Span::styled(fi.nav_char.clone(), style);
                frame.render_widget(Paragraph::new(span), indicator_rect);
            } else {
                let indicator = Block::default()
                    .borders(Borders::LEFT)
                    .border_type(border_type)
                    .border_style(style);
                frame.render_widget(indicator, area);
            }
        }
    }

    p
}

fn render_status(frame: &mut Frame, area: Rect, ui: &ResultsUI, full_width: u16) {
    if ui.status_config.show {
        let widget = ui.make_status(full_width);
        frame.render_widget(widget, area);
    }
}

fn render_display(frame: &mut Frame, area: Rect, ui: &mut DisplayUI, results_ui: &ResultsUI) {
    if !ui.show {
        return;
    }
    let widths = results_ui.widths().to_vec();

    let widget = ui.make_display(
        results_ui.indentation() as u16 + results_ui.config.border.left(),
        widths,
        results_ui.config.column_spacing.0,
    );

    frame.render_widget(widget, area);

    if ui.is_single_column() {
        let widget = ui.make_full_width_row(results_ui.indentation() as u16);
        frame.render_widget(widget, area);
    }
}

const SORT_MENU_ITEMS: &[(&str, &str, ratatui::style::Color)] = &[
    ("[a]", "Alpha", ratatui::style::Color::Cyan),
    ("[A]", "Alpha (rev)", ratatui::style::Color::Cyan),
    ("[n]", "Natural", ratatui::style::Color::Green),
    ("[N]", "Natural (rev)", ratatui::style::Color::Green),
    ("[m]", "Mtime", ratatui::style::Color::Yellow),
    ("[M]", "Mtime (rev)", ratatui::style::Color::Yellow),
    ("[b]", "Btime", ratatui::style::Color::LightGreen),
    ("[B]", "Btime (rev)", ratatui::style::Color::LightGreen),
    ("[s]", "Size", ratatui::style::Color::Magenta),
    ("[S]", "Size (rev)", ratatui::style::Color::Magenta),
    ("[e]", "Ext", ratatui::style::Color::Blue),
    ("[E]", "Ext (rev)", ratatui::style::Color::Blue),
    ("[Esc]", "Cancel", ratatui::style::Color::DarkGray),
];

fn render_sort_menu(frame: &mut Frame, area: Rect, cfg: &crate::config::SortMenuConfig) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    if cfg.columns <= 1 {
        let mut spans = Vec::new();
        let mut total_w = 0;
        if cfg.show_title {
            let title_span = Span::styled(
                " Sort: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            total_w += title_span.width();
            spans.push(title_span);
        }
        let max_w = area.width as usize;

        for (key, label, color) in SORT_MENU_ITEMS {
            let key_span = Span::styled(
                format!("{key}"),
                Style::default().fg(*color).add_modifier(Modifier::BOLD),
            );
            let label_span = Span::styled(format!(" {label} "), Style::default().fg(Color::White));
            let pair_w = key_span.width() + label_span.width();
            if total_w + pair_w > max_w {
                break;
            }
            total_w += pair_w;
            spans.push(key_span);
            spans.push(label_span);
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    } else {
        let cols = cfg.columns;
        let col_w = (area.width as usize) / cols;
        if col_w == 0 {
            return;
        }

        let num_rows = (SORT_MENU_ITEMS.len() + cols - 1) / cols;
        let mut lines = Vec::with_capacity(num_rows);

        for row in 0..num_rows {
            let mut line_spans = Vec::new();
            for col in 0..cols {
                let idx = row * cols + col;
                if idx < SORT_MENU_ITEMS.len() {
                    let (key, label, color) = SORT_MENU_ITEMS[idx];
                    let key_span = Span::styled(
                        format!(" {key}"),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    );
                    let label_span =
                        Span::styled(format!(" {label}"), Style::default().fg(Color::White));
                    let used_w = key_span.width() + label_span.width();
                    line_spans.push(key_span);
                    line_spans.push(label_span);
                    if col + 1 < cols && used_w < col_w {
                        line_spans.push(Span::raw(" ".repeat(col_w - used_w)));
                    }
                }
            }
            lines.push(Line::from(line_spans));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }
}

pub const BASIC_NAV_HINTS: &[(&str, &str, ratatui::style::Color)] = &[
    ("[/]", "Filter", ratatui::style::Color::Cyan),
    ("[j/k]", "Move", ratatui::style::Color::Yellow),
    ("[h/l]", "Up/Dir", ratatui::style::Color::Yellow),
    ("[C-h/l]", "Trav", ratatui::style::Color::Green),
    ("[C-p]", "Preview", ratatui::style::Color::Blue),
    ("[J/K]", "Scroll", ratatui::style::Color::Blue),
    ("[,]", "Sort", ratatui::style::Color::Yellow),
    ("[\\]", "Pane", ratatui::style::Color::Cyan),
];

pub const NAV_HINTS: &[(&str, &str, ratatui::style::Color)] = &[
    ("[/]", "Filter", ratatui::style::Color::Cyan),
    ("[Space]", "Select", ratatui::style::Color::Yellow),
    ("[,]", "Sort", ratatui::style::Color::Yellow),
    ("[f]", "Frecency", ratatui::style::Color::Cyan),
    ("[b]", "Bookmarks", ratatui::style::Color::Magenta),
    ("[*]", "Bookmark", ratatui::style::Color::Yellow),
    ("[e]", "Edit", ratatui::style::Color::Green),
    ("[a]", "Add", ratatui::style::Color::Green),
    ("[r]", "Rename", ratatui::style::Color::Yellow),
    ("[d]", "Trash", ratatui::style::Color::Red),
    ("[y/x]", "Copy/Cut", ratatui::style::Color::Green),
    ("[p/P]", "Paste", ratatui::style::Color::Magenta),
    ("[u]", "Undo", ratatui::style::Color::Blue),
    ("[z/Z]", "Zip", ratatui::style::Color::Blue),
    ("[\\]", "Pane", ratatui::style::Color::Cyan),
    ("[C-p]", "Preview", ratatui::style::Color::Blue),
];

pub const PREVIEW_NAV_HINTS: &[(&str, &str, ratatui::style::Color)] = &[
    ("[hjkl]", "Pan", ratatui::style::Color::Yellow),
    ("[J/K]", "Jump15", ratatui::style::Color::Yellow),
    ("[C-u/u]", "HalfUp", ratatui::style::Color::Yellow),
    ("[C-d/d]", "HalfDn", ratatui::style::Color::Yellow),
    ("[s/m]", "Diagram", ratatui::style::Color::Magenta),
    ("[n/N]", "DiagIdx", ratatui::style::Color::Cyan),
    ("[+/-]", "Zoom", ratatui::style::Color::Green),
    ("[0/z]", "Reset", ratatui::style::Color::Blue),
    ("[esc/enter]", "Back", ratatui::style::Color::Red),
];

fn render_nav_hints(
    frame: &mut Frame,
    area: Rect,
    is_basic: bool,
    columns: usize,
    preview_fullscreen: bool,
    zoom_pct: Option<u32>,
) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    let hints: &[(&str, &str, Color)] = if preview_fullscreen {
        PREVIEW_NAV_HINTS
    } else if is_basic {
        BASIC_NAV_HINTS
    } else {
        NAV_HINTS
    };

    let cols = if columns == 0 { 4 } else { columns };

    if cols <= 1 {
        let mut spans = Vec::new();
        let mut total_w = 0;
        let max_w = area.width as usize;

        for (key, label, color) in hints {
            let label_owned = if preview_fullscreen && *key == "[+/-]" && let Some(pct) = zoom_pct {
                format!("Zoom {pct}%")
            } else {
                label.to_string()
            };
            let key_span = Span::styled(
                format!(" {key}"),
                Style::default().fg(*color).add_modifier(Modifier::BOLD),
            );
            let label_span =
                Span::styled(format!(" {label_owned} "), Style::default().fg(Color::DarkGray));
            let pair_w = key_span.width() + label_span.width();
            if total_w + pair_w > max_w {
                break;
            }
            total_w += pair_w;
            spans.push(key_span);
            spans.push(label_span);
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    } else {
        let col_w = (area.width as usize) / cols;
        if col_w == 0 {
            return;
        }

        let num_rows = (hints.len() + cols - 1) / cols;
        let mut lines = Vec::with_capacity(num_rows);

        for row in 0..num_rows {
            let mut line_spans = Vec::new();
            for col in 0..cols {
                let idx = row * cols + col;
                if idx < hints.len() {
                    let (key, label, color) = hints[idx];
                    let label_owned = if preview_fullscreen && key == "[+/-]" && let Some(pct) = zoom_pct {
                        format!("Zoom {pct}%")
                    } else {
                        label.to_string()
                    };
                    let key_span = Span::styled(
                        format!(" {key}"),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    );
                    let label_span =
                        Span::styled(format!(" {label_owned}"), Style::default().fg(Color::White));
                    let used_w = key_span.width() + label_span.width();
                    line_spans.push(key_span);
                    line_spans.push(label_span);
                    if col + 1 < cols && used_w < col_w {
                        line_spans.push(Span::raw(" ".repeat(col_w - used_w)));
                    }
                }
            }
            lines.push(Line::from(line_spans));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }
}

fn render_parent_peek(frame: &mut Frame, area: Rect, cfg: &crate::config::ParentPeekConfig) {
    if area.height <= 2 || area.width <= 2 {
        return;
    }

    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let Some(parent) = cwd.parent() else { return };

    let parent_name = parent
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_else(|| "/".into());

    let current_name = cwd.file_name().map(|n| n.to_string_lossy());

    let parent_color = cfg.parent_color.unwrap_or(Color::Cyan);

    let inner = if cfg.border.show {
        let border_color = cfg.border.color.unwrap_or(Color::DarkGray);
        let border_type = cfg
            .border
            .r#type
            .unwrap_or(ratatui::widgets::BorderType::Plain);

        let block = Block::default()
            .borders(Borders::RIGHT)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" {} ", parent_name),
                Style::default()
                    .fg(parent_color)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);
        inner_area
    } else {
        area
    };

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let mut entries = Vec::new();
    if let Ok(dir_entries) = std::fs::read_dir(parent) {
        for entry in dir_entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map_or(false, |t| t.is_dir());
            entries.push((name, is_dir));
        }
    }

    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut selected_idx = 0;
    if let Some(ref cur) = current_name {
        if let Some(pos) = entries.iter().position(|e| e.0 == *cur) {
            selected_idx = pos;
        }
    }

    let visible_rows = inner.height as usize;
    let half_visible = visible_rows / 2;
    let start_idx = selected_idx.saturating_sub(half_visible);
    let end_idx = (start_idx + visible_rows).min(entries.len());

    let highlight_color = cfg.highlight_color.unwrap_or(Color::Yellow);

    let mut lines = Vec::new();
    for (idx, (name, is_dir)) in entries[start_idx..end_idx].iter().enumerate() {
        let actual_idx = start_idx + idx;
        let is_selected = (actual_idx == selected_idx) && cfg.highlight;

        let icon = if *is_dir { " " } else { " " };
        let text = format!("{}{}", icon, name);

        let style = if is_selected {
            Style::default()
                .fg(highlight_color)
                .add_modifier(Modifier::BOLD)
        } else if *is_dir {
            Style::default().fg(Color::Blue)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        lines.push(Line::from(Span::styled(text, style)));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

// a bit weird, do we want mutable, do we want &mut ui, whatever this is simplest
fn render_ui(frame: &mut Frame, area: &mut Rect, ui: &UI) {
    if ui.config.border.sides.is_some() {
        let widget = ui.make_ui();
        frame.render_widget(widget, *area);
    }
    *area = ui.compute_area(area);
}

fn split(rect: &mut Rect, height: u16, cut_top: bool) -> Rect {
    let h = height.min(rect.height);

    if cut_top {
        let offshoot = Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: h,
        };

        rect.y += h;
        rect.height -= h;

        offshoot
    } else {
        let offshoot = Rect {
            x: rect.x,
            y: rect.y + rect.height - h,
            width: rect.width,
            height: h,
        };

        rect.height -= h;

        offshoot
    }
}

// -----------------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use crate::action::{Action, Actions};
    use crate::message::RenderCommand;
    use std::collections::HashMap;

    #[test]
    fn test_apply_focus_binds_sequences() {
        use crate::action::NullActionExt;

        let mut focus_binds = HashMap::new();
        focus_binds.insert("gg".to_string(), Actions::from([Action::PreviewUp(0)]));
        focus_binds.insert("G".to_string(), Actions::from([Action::PreviewDown(0)]));
        focus_binds.insert("J".to_string(), Actions::from([Action::PreviewDown(1)]));
        focus_binds.insert("K".to_string(), Actions::from([Action::PreviewUp(1)]));
        focus_binds.insert("gb".to_string(), Actions::from([Action::Pos(-1)]));
        focus_binds.insert("gt".to_string(), Actions::from([Action::Pos(0)]));

        let mut pending = None;
        let mut sort_menu_active = false;

        // Test gg
        let mut buffer = vec![
            RenderCommand::<NullActionExt>::Action(Action::Char('g')),
            RenderCommand::<NullActionExt>::Action(Action::Char('g')),
        ];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::PreviewUp(0))
        ));
        assert_eq!(pending, None);

        // Test G
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('G'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::PreviewDown(0))
        ));
        assert_eq!(pending, None);

        // Test J
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('J'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::PreviewDown(1))
        ));
        assert_eq!(pending, None);

        // Test K
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('K'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::PreviewUp(1))
        ));
        assert_eq!(pending, None);

        // Test gt
        let mut buffer = vec![
            RenderCommand::<NullActionExt>::Action(Action::Char('g')),
            RenderCommand::<NullActionExt>::Action(Action::Char('t')),
        ];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::Pos(0))));
        assert_eq!(pending, None);

        // Test gb
        let mut buffer = vec![
            RenderCommand::<NullActionExt>::Action(Action::Char('g')),
            RenderCommand::<NullActionExt>::Action(Action::Char('b')),
        ];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::Pos(-1))));
        assert_eq!(pending, None);

        // Test ',' opens sort menu
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char(','))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::SortMenu)));
        assert!(sort_menu_active);

        // Test next key 'n' in sort menu dispatches Action::Sort(Natural) and closes menu
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('n'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::Sort(Some(crate::action::SortOrder::Natural)))
        ));
        assert!(!sort_menu_active);

        // Test next key 'b' in sort menu dispatches Action::Sort(Created) and closes menu
        sort_menu_active = true;
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('b'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::Sort(Some(crate::action::SortOrder::Created)))
        ));
        assert!(!sort_menu_active);

        // Test ',' then 'Esc' cancels without dispatching sort
        sort_menu_active = true;
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Quit(1))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 0);
        assert!(!sort_menu_active);
    }

    #[test]
    fn test_apply_focus_binds_key_actions() {
        use crate::action::NullActionExt;

        let mut focus_binds = HashMap::new();
        focus_binds.insert("esc".to_string(), Actions::from([Action::Quit(1)]));
        focus_binds.insert("q".to_string(), Actions::from([Action::Quit(1)]));
        focus_binds.insert("/".to_string(), Actions::from([Action::FocusFilter]));
        focus_binds.insert(
            "backspace".to_string(),
            Actions::from([
                Action::ChDir("..".to_string()),
                Action::Cancel,
                Action::Reload("".to_string()),
                Action::FocusNav,
            ]),
        );

        let mut pending = None;
        let mut sort_menu_active = false;

        // 1. Esc in Focus::Results with "esc" = "Quit" in nav_binds must emit Action::Quit(1)
        // Works both with lowercase "esc" and crokey's capitalized "Esc"
        for esc_key in ["esc", "Esc"] {
            let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
                key: esc_key.to_string(),
                action: Action::ToggleFocus,
            }];
            apply_focus_binds(
                &mut buffer,
                Focus::Results,
                &focus_binds,
                false,
                &mut pending,
                &mut sort_menu_active,
            false,
            );
            assert_eq!(buffer.len(), 1);
            assert!(matches!(buffer[0], RenderCommand::Action(Action::Quit(1))));
        }

        // 2. Esc in Focus::Input must toggle focus to Results
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "Esc".to_string(),
            action: Action::ToggleFocus,
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Input,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::ToggleFocus)
        ));

        // 3. Backspace in Focus::Results must run custom nav_binds (case-insensitive)
        for bs_key in ["backspace", "Backspace"] {
            let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
                key: bs_key.to_string(),
                action: Action::DeleteChar,
            }];
            apply_focus_binds(
                &mut buffer,
                Focus::Results,
                &focus_binds,
                false,
                &mut pending,
                &mut sort_menu_active,
            false,
            );
            assert_eq!(buffer.len(), 4);
            assert!(matches!(buffer[0], RenderCommand::Action(Action::ChDir(_))));
            assert!(matches!(buffer[1], RenderCommand::Action(Action::Cancel)));
            assert!(matches!(
                buffer[2],
                RenderCommand::Action(Action::Reload(_))
            ));
            assert!(matches!(buffer[3], RenderCommand::Action(Action::FocusNav)));
        }

        // 4. Backspace in Focus::Input must delete char
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "Backspace".to_string(),
            action: Action::DeleteChar,
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Input,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::DeleteChar)
        ));

        // 5. Esc in Focus::Results WITHOUT "esc" in nav_binds must quit with 130 instead of trapping focus
        for esc_key in ["esc", "Esc"] {
            let empty_binds = HashMap::new();
            let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
                key: esc_key.to_string(),
                action: Action::ToggleFocus,
            }];
            apply_focus_binds(
                &mut buffer,
                Focus::Results,
                &empty_binds,
                false,
                &mut pending,
                &mut sort_menu_active,
            false,
            );
            assert_eq!(buffer.len(), 1);
            assert!(matches!(
                buffer[0],
                RenderCommand::Action(Action::Quit(130))
            ));
        }
    }

    #[test]
    fn test_apply_focus_binds_esc_and_nav() {
        use crate::action::{Actions, NullActionExt};
        let mut focus_binds = std::collections::HashMap::new();
        focus_binds.insert("esc".to_string(), Actions::from([Action::Quit(0)]));
        focus_binds.insert("q".to_string(), Actions::from([Action::Quit(0)]));
        focus_binds.insert("/".to_string(), Actions::from([Action::FocusFilter]));
        focus_binds.insert("backspace".to_string(), Actions::from([Action::Pos(0)]));

        let mut pending = None;
        let mut sort_menu_active = false;

        // 1. In Input mode, ToggleFocus passes through
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::ToggleFocus)];
        apply_focus_binds(
            &mut buffer,
            Focus::Input,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::ToggleFocus)
        ));

        // 2. In Results mode, ToggleFocus is intercepted and replaced by nav_binds["esc"] (Quit)
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::ToggleFocus)];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::Quit(0))));

        // 3. In Results mode, 'q' triggers Quit
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('q'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::Quit(0))));

        // 4. In Results mode, '/' triggers FocusFilter
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('/'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(
            buffer[0],
            RenderCommand::Action(Action::FocusFilter)
        ));

        // 5. In Results mode, DeleteChar triggers 'backspace' bind
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::DeleteChar)];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::Pos(0))));
    }

    #[test]
    fn test_apply_focus_binds_tab_reloadnext_no_multiplication() {
        use crate::action::{Actions, NullActionExt};
        let mut focus_binds = std::collections::HashMap::new();
        focus_binds.insert(
            "tab".to_string(),
            Actions::from([Action::Semantic("reloadnext".to_string())]),
        );

        let mut pending = None;
        let mut sort_menu_active = false;

        // 1. Traces surrounding a KeyAction for "Tab" should not duplicate the nav_bind
        let mut buffer = vec![
            RenderCommand::<NullActionExt>::Action(Action::Trace("@@reloadnext".to_string())),
            RenderCommand::<NullActionExt>::KeyAction {
                key: "Tab".to_string(),
                action: Action::Semantic("reloadnext".to_string()),
            },
            RenderCommand::<NullActionExt>::Action(Action::Trace(String::new())),
        ];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        let reloadnext_count = buffer
            .iter()
            .filter(|cmd| match cmd {
                RenderCommand::Action(Action::Semantic(s)) => s == "reloadnext",
                _ => false,
            })
            .count();
        assert_eq!(
            reloadnext_count, 1,
            "Tab in Focus::Results must trigger reloadnext exactly once"
        );

        // 2. Multiple consecutive KeyActions with same key (e.g. from multi-action bind)
        // must execute the nav_bind only once
        let mut buffer = vec![
            RenderCommand::<NullActionExt>::KeyAction {
                key: "Tab".to_string(),
                action: Action::Toggle,
            },
            RenderCommand::<NullActionExt>::KeyAction {
                key: "Tab".to_string(),
                action: Action::Down(1),
            },
        ];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        let reloadnext_count = buffer
            .iter()
            .filter(|cmd| match cmd {
                RenderCommand::Action(Action::Semantic(s)) => s == "reloadnext",
                _ => false,
            })
            .count();
        assert_eq!(
            reloadnext_count, 1,
            "Multi-action Tab must not duplicate nav_bind execution"
        );

        // 3. Unmapped multi-action key in Focus::Results preserves all fallback actions
        let mut buffer = vec![
            RenderCommand::<NullActionExt>::KeyAction {
                key: "alt-l".to_string(),
                action: Action::Cancel,
            },
            RenderCommand::<NullActionExt>::KeyAction {
                key: "alt-l".to_string(),
                action: Action::Pos(0),
            },
        ];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(
            buffer.len(),
            2,
            "Unmapped key must preserve all fallback actions"
        );
        assert!(matches!(buffer[0], RenderCommand::Action(Action::Cancel)));
        assert!(matches!(buffer[1], RenderCommand::Action(Action::Pos(0))));

        // 4. In Focus::Input, Tab executes the action directly
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "Tab".to_string(),
            action: Action::Semantic("reloadnext".to_string()),
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Input,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert!(
            matches!(buffer[0], RenderCommand::Action(Action::Semantic(ref s)) if s == "reloadnext")
        );
    }

    #[test]
    fn test_nav_hints_grid_height_and_columns() {
        use crate::config::UiConfig;
        let mut ui = UiConfig::default();
        assert!(ui.nav_hints);
        assert_eq!(ui.nav_hints_columns, 4);

        // In 4 columns:
        // Full hints (16 items) -> (16 + 4 - 1) / 4 = 4 rows
        assert_eq!(ui.nav_hints_height(NAV_HINTS.len()), 4);

        // Basic hints (8 items) -> (8 + 4 - 1) / 4 = 2 rows
        assert_eq!(ui.nav_hints_height(BASIC_NAV_HINTS.len()), 2);

        // If columns configured to 1 -> 1 row
        ui.nav_hints_columns = 1;
        assert_eq!(ui.nav_hints_height(NAV_HINTS.len()), 1);

        // If nav_hints disabled -> 0 rows
        ui.nav_hints = false;
        assert_eq!(ui.nav_hints_height(NAV_HINTS.len()), 0);
    }

    #[test]
    fn test_preview_fullscreen_key_navigation() {
        use crate::action::NullActionExt;
        let focus_binds = std::collections::HashMap::new();
        let mut pending = None;
        let mut sort_menu_active = false;

        // Esc in Results when preview_fullscreen is true -> CyclePreview
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "esc".to_string(),
            action: Action::ToggleFocus,
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::CyclePreview)));

        // Esc in Input when preview_fullscreen is true -> CyclePreview
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "esc".to_string(),
            action: Action::ToggleFocus,
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Input,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::CyclePreview)));

        // ToggleFocus action directly when preview_fullscreen is true -> CyclePreview
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::ToggleFocus)];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::CyclePreview)));

        // j / k when preview_fullscreen is true -> PreviewDown(3) / PreviewUp(3)
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "j".to_string(),
            action: Action::Down(1),
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewDown(3))));

        // h / l when preview_fullscreen is true -> PreviewHScroll(-6) / PreviewHScroll(6)
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('h'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewHScroll(-6))));

        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('l'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewHScroll(6))));

        // d when preview_fullscreen is true -> PreviewHalfPageDown
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('d'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewHalfPageDown)));

        // ctrl-d when preview_fullscreen is true -> PreviewHalfPageDown
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "ctrl-d".to_string(),
            action: Action::PreviewHalfPageDown,
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewHalfPageDown)));

        // u and ctrl-u when preview_fullscreen is true -> PreviewHalfPageUp
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('u'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewHalfPageUp)));

        // s when preview_fullscreen is true -> ToggleDiagram
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('s'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::ToggleDiagram)));

        // enter when preview_fullscreen is true -> CyclePreview
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "enter".to_string(),
            action: Action::CyclePreview,
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::CyclePreview)));

        // y when preview_fullscreen is true -> Accept
        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "y".to_string(),
            action: Action::Char('y'),
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::Accept)));

        // j / k in Focus::Input when preview_fullscreen is true -> PreviewDown(3) / PreviewUp(3)
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('j'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Input,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewDown(3))));

        let mut buffer = vec![RenderCommand::<NullActionExt>::KeyAction {
            key: "k".to_string(),
            action: Action::Char('k'),
        }];
        apply_focus_binds(
            &mut buffer,
            Focus::Input,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewUp(3))));

        // Zoom keys when preview_fullscreen is true
        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('+'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::DiagramZoomIn)));

        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('-'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::DiagramZoomOut)));

        // gg and G scroll jumps when preview_fullscreen is true
        let mut buffer = vec![
            RenderCommand::<NullActionExt>::Action(Action::Char('g')),
            RenderCommand::<NullActionExt>::Action(Action::Char('g')),
        ];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewUp(0))));

        let mut buffer = vec![RenderCommand::<NullActionExt>::Action(Action::Char('G'))];
        apply_focus_binds(
            &mut buffer,
            Focus::Results,
            &focus_binds,
            false,
            &mut pending,
            &mut sort_menu_active,
            true,
        );
        assert_eq!(buffer.len(), 1);
        assert!(matches!(buffer[0], RenderCommand::Action(Action::PreviewDown(0))));
    }
}

// #[cfg(test)]
// async fn send_every_second(tx: mpsc::UnboundedSender<RenderCommand>) {
//     let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

//     loop {
//         interval.tick().await;
//         if tx.send(RenderCommand::quit()).is_err() {
//             break;
//         }
//     }
// }
