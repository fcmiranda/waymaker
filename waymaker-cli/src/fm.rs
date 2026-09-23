//! File-manager overlays and clipboard state for navigation mode.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

use waymaker::{
    Action,
    event::RenderSender,
    message::RenderCommand,
    ui::{Frame, Overlay, OverlayEffect, Rect, SizeHint},
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::action::MMAction;

/// The name of the item currently under the cursor (first column only).
pub type CurrentItem = Arc<Mutex<Option<String>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipOp {
    Copy,
    Cut,
}

#[derive(Clone, Debug)]
pub struct FmClipboard {
    pub items: Vec<PathBuf>,
    pub op: ClipOp,
}

pub type Clipboard = Arc<Mutex<Option<FmClipboard>>>;

#[derive(Debug, Clone)]
pub enum UndoAction {
    DeletedFile {
        original: PathBuf,
        backup: PathBuf,
    },
    DeletedFiles {
        items: Vec<(PathBuf, PathBuf)>,
    },
    CreatedFile {
        path: PathBuf,
    },
    Renamed {
        from: PathBuf,
        to: PathBuf,
    },
    Copied {
        dest: PathBuf,
    },
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    Paste {
        op: ClipOp,
        items: Vec<(PathBuf, PathBuf)>,
        saved_clipboard: Option<FmClipboard>,
        saved_yank_paths: std::collections::HashSet<String>,
        saved_cut_paths: std::collections::HashSet<String>,
        previous_dir: Option<PathBuf>,
        target_dir: Option<PathBuf>,
    },
}

pub type UndoStack = Arc<Mutex<Vec<UndoAction>>>;

fn get_deletion_date() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days = secs / 86400;
    let sec_in_day = secs % 86400;

    let hour = sec_in_day / 3600;
    let min = (sec_in_day % 3600) / 60;
    let sec = sec_in_day % 60;

    let mut year = 1970;
    let mut days_left = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if days_left < days_in_year {
            break;
        }
        days_left -= days_in_year;
        year += 1;
    }

    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for &d in &month_days {
        if days_left < d {
            break;
        }
        days_left -= d;
        month += 1;
    }
    let day = days_left + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hour, min, sec
    )
}

fn percent_encode(s: &str) -> String {
    let mut res = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                res.push(b as char);
            }
            _ => {
                res.push_str(&format!("%{:02X}", b));
            }
        }
    }
    res
}

pub fn move_to_trash(path: &Path) -> std::io::Result<PathBuf> {
    let trash_dir = dirs::data_local_dir()
        .map(|d| d.join("Trash"))
        .unwrap_or_else(|| std::env::temp_dir().join("mm_trash"));

    let files_dir = trash_dir.join("files");
    let info_dir = trash_dir.join("info");

    fs::create_dir_all(&files_dir)?;
    fs::create_dir_all(&info_dir)?;

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "item".to_string());

    let mut backup = files_dir.join(&name);
    let mut info_file = info_dir.join(format!("{}.trashinfo", name));
    if backup.exists() {
        let mut n = 1u32;
        loop {
            let candidate_name = format!("{}_{}", n, name);
            let candidate_backup = files_dir.join(&candidate_name);
            let candidate_info = info_dir.join(format!("{}.trashinfo", candidate_name));
            if !candidate_backup.exists() {
                backup = candidate_backup;
                info_file = candidate_info;
                break;
            }
            n += 1;
        }
    }

    let absolute_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    move_path(path, &backup)?;

    let path_str = absolute_path.to_string_lossy();
    let encoded_path = percent_encode(&path_str);
    let date_str = get_deletion_date();
    let info_content = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encoded_path, date_str
    );

    fs::write(info_file, info_content)?;

    Ok(backup)
}

pub fn copy_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        copy_dir_all(src, dst)
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

pub fn apply_undo(action: &UndoAction) -> std::io::Result<()> {
    match action {
        UndoAction::DeletedFile { original, backup } => {
            move_path(backup, original)?;
            if let Some(name) = backup.file_name() {
                if let Some(parent) = backup.parent() {
                    if let Some(grandparent) = parent.parent() {
                        let info_file = grandparent
                            .join("info")
                            .join(format!("{}.trashinfo", name.to_string_lossy()));
                        let _ = fs::remove_file(info_file);
                    }
                }
            }
            Ok(())
        }
        UndoAction::DeletedFiles { items } => {
            for (original, backup) in items {
                let _ = move_path(backup, original);
                if let Some(name) = backup.file_name() {
                    if let Some(parent) = backup.parent() {
                        if let Some(grandparent) = parent.parent() {
                            let info_file = grandparent
                                .join("info")
                                .join(format!("{}.trashinfo", name.to_string_lossy()));
                            let _ = fs::remove_file(info_file);
                        }
                    }
                }
            }
            Ok(())
        }
        UndoAction::CreatedFile { path } => {
            if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
        }
        UndoAction::Renamed { from, to } => move_path(to, from),
        UndoAction::Copied { dest } => {
            if dest.is_dir() {
                fs::remove_dir_all(dest)
            } else {
                fs::remove_file(dest)
            }
        }
        UndoAction::Moved { from, to } => move_path(to, from),
        UndoAction::Paste { op, items, .. } => {
            for (_src, dest) in items {
                match op {
                    ClipOp::Copy => {
                        if dest.is_dir() {
                            let _ = fs::remove_dir_all(dest);
                        } else {
                            let _ = fs::remove_file(dest);
                        }
                    }
                    ClipOp::Cut => {
                        let _ = move_path(dest, _src);
                    }
                }
            }
            Ok(())
        }
    }
}

pub fn apply_redo(action: &UndoAction) -> std::io::Result<()> {
    match action {
        UndoAction::DeletedFile { original, .. } => {
            let _ = move_to_trash(original)?;
            Ok(())
        }
        UndoAction::DeletedFiles { items } => {
            for (original, _) in items {
                let _ = move_to_trash(original);
            }
            Ok(())
        }
        UndoAction::CreatedFile { path } => {
            if path.to_string_lossy().ends_with('/') {
                fs::create_dir_all(path)
            } else {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                {
                    let _ = fs::create_dir_all(parent);
                }
                fs::File::create(path).map(|_| ())
            }
        }
        UndoAction::Renamed { from, to } => move_path(from, to),
        UndoAction::Copied { dest: _ } => Ok(()),
        UndoAction::Moved { from, to } => move_path(from, to),
        UndoAction::Paste { op, items, .. } => {
            for (src, dest) in items {
                match op {
                    ClipOp::Copy => {
                        copy_path(src, dest)?;
                    }
                    ClipOp::Cut => {
                        move_path(src, dest)?;
                    }
                }
            }
            Ok(())
        }
    }
}

pub struct DeleteOverlay {
    current: CurrentItem,
    name: String,
    tx: RenderSender<MMAction>,
    undo_stack: UndoStack,
}

impl DeleteOverlay {
    pub fn new(current: CurrentItem, tx: RenderSender<MMAction>, undo_stack: UndoStack) -> Self {
        Self {
            current,
            name: String::new(),
            tx,
            undo_stack,
        }
    }

    fn do_delete(&mut self) -> OverlayEffect {
        let path = PathBuf::from(&self.name);
        match move_to_trash(&path) {
            Ok(backup) => {
                if let Ok(mut stack) = self.undo_stack.lock() {
                    stack.push(UndoAction::DeletedFile {
                        original: path,
                        backup,
                    });
                }
            }
            Err(e) => log::error!("fm delete '{}': {e}", path.display()),
        }
        let _ = self
            .tx
            .send(RenderCommand::Action(Action::Reload(String::new())));
        OverlayEffect::Disable
    }
}

impl Overlay for DeleteOverlay {
    type A = MMAction;

    fn on_enable(&mut self, _area: &Rect) {
        self.name = self
            .current
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default();
    }

    fn handle_input(&mut self, c: char) -> OverlayEffect {
        match c {
            'y' | 'Y' => self.do_delete(),
            _ => OverlayEffect::Disable,
        }
    }

    fn handle_action(&mut self, action: &Action<MMAction>) -> OverlayEffect {
        match action {
            Action::Accept | Action::Custom(MMAction::Accept) => self.do_delete(),
            Action::Quit(_)
            | Action::Cancel
            | Action::ToggleFocus
            | Action::FocusFilter
            | Action::FocusNav => OverlayEffect::Disable,
            _ => OverlayEffect::None,
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let label = format!("  '{}' ? [y/N] ", self.name);
        let para = Paragraph::new(Line::from(vec![
            Span::styled(
                "  ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw("'"),
            Span::styled(&self.name, Style::default().fg(Color::Yellow)),
            Span::raw("'"),
            Span::styled(" ? [y/N] ", Style::default().fg(Color::DarkGray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .title(Span::styled(
                    "  ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
        );
        let _ = label;
        frame.render_widget(Clear, area);
        frame.render_widget(para, area);
    }

    fn area(&mut self, ui_area: &Rect) -> Result<Rect, [SizeHint; 2]> {
        let w = (self.name.len() as u16 + 20)
            .max(30)
            .min(ui_area.width.saturating_sub(2));
        Ok(Rect {
            x: ui_area.x + (ui_area.width.saturating_sub(w)) / 2,
            y: ui_area.y + ui_area.height.saturating_sub(3),
            width: w,
            height: 3,
        })
    }
}

pub struct CreateOverlay {
    input: String,
    dialog_width: u16,
    tx: RenderSender<MMAction>,
    undo_stack: UndoStack,
}

impl CreateOverlay {
    pub fn new(tx: RenderSender<MMAction>, undo_stack: UndoStack) -> Self {
        Self {
            input: String::new(),
            dialog_width: 40,
            tx,
            undo_stack,
        }
    }

    fn commit(&mut self) -> OverlayEffect {
        let name = self.input.trim().to_string();
        if name.is_empty() {
            return OverlayEffect::Disable;
        }
        let result = if name.ends_with('/') {
            fs::create_dir_all(&name)
        } else {
            if let Some(parent) = Path::new(&name).parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = fs::create_dir_all(parent);
                }
            }
            fs::File::create(&name).map(|_| ())
        };
        if let Err(e) = result {
            log::error!("fm create '{name}': {e}");
        } else if let Ok(mut stack) = self.undo_stack.lock() {
            stack.push(UndoAction::CreatedFile {
                path: PathBuf::from(&name),
            });
        }
        let _ = self
            .tx
            .send(RenderCommand::Action(Action::Reload(String::new())));
        OverlayEffect::Disable
    }
}

impl Overlay for CreateOverlay {
    type A = MMAction;

    fn on_enable(&mut self, area: &Rect) {
        self.input.clear();
        self.dialog_width = (area.width / 2).max(40).min(area.width.saturating_sub(4));
    }

    fn handle_input(&mut self, c: char) -> OverlayEffect {
        self.input.push(c);
        OverlayEffect::None
    }

    fn handle_action(&mut self, action: &Action<MMAction>) -> OverlayEffect {
        match action {
            Action::Accept | Action::Custom(MMAction::Accept) => self.commit(),
            Action::DeleteChar => {
                self.input.pop();
                OverlayEffect::None
            }
            Action::DeleteWord => {
                let trimmed = self.input.trim_end();
                let last_space = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                self.input.truncate(last_space);
                OverlayEffect::None
            }
            Action::Cancel
            | Action::Quit(_)
            | Action::ToggleFocus
            | Action::FocusFilter
            | Action::FocusNav => OverlayEffect::Disable,
            _ => OverlayEffect::None,
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let hint = if self.input.ends_with('/') {
            " (folder)"
        } else {
            " (file)"
        };
        const PROMPT: &str = "  ";
        let inner_w = (area.width.saturating_sub(2 + PROMPT.len() as u16)) as usize;
        let visible = visible_suffix(&self.input, inner_w);
        let para = Paragraph::new(Line::from(vec![
            Span::styled(
                PROMPT,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(visible.to_string(), Style::default().fg(Color::White)),
            Span::styled(hint, Style::default().fg(Color::DarkGray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(Span::styled(
                    "  ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        frame.render_widget(Clear, area);
        frame.render_widget(para, area);
    }

    fn area(&mut self, ui_area: &Rect) -> Result<Rect, [SizeHint; 2]> {
        let w = self
            .dialog_width
            .max(30)
            .min(ui_area.width.saturating_sub(2));
        Ok(Rect {
            x: ui_area.x + (ui_area.width.saturating_sub(w)) / 2,
            y: ui_area.y + ui_area.height.saturating_sub(3),
            width: w,
            height: 3,
        })
    }
}

pub struct RenameOverlay {
    current: CurrentItem,
    original: String,
    input: String,
    dialog_width: u16,
    tx: RenderSender<MMAction>,
    undo_stack: UndoStack,
}

impl RenameOverlay {
    pub fn new(current: CurrentItem, tx: RenderSender<MMAction>, undo_stack: UndoStack) -> Self {
        Self {
            current,
            original: String::new(),
            input: String::new(),
            dialog_width: 40,
            tx,
            undo_stack,
        }
    }

    fn commit(&mut self) -> OverlayEffect {
        let new_name = self.input.trim().to_string();
        if new_name.is_empty() || new_name == self.original {
            return OverlayEffect::Disable;
        }
        if let Err(e) = move_path(Path::new(&self.original), Path::new(&new_name)) {
            log::error!("fm rename '{}' -> '{new_name}': {e}", self.original);
        } else if let Ok(mut stack) = self.undo_stack.lock() {
            stack.push(UndoAction::Renamed {
                from: PathBuf::from(&self.original),
                to: PathBuf::from(&new_name),
            });
        }
        let _ = self
            .tx
            .send(RenderCommand::Action(Action::Reload(String::new())));
        OverlayEffect::Disable
    }
}

impl Overlay for RenameOverlay {
    type A = MMAction;

    fn on_enable(&mut self, area: &Rect) {
        self.original = self
            .current
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default();
        self.input = self.original.trim_end_matches('/').to_string();
        let for_initial = input_width(&self.input);
        let half = area.width / 2;
        self.dialog_width = for_initial
            .max(half)
            .max(40)
            .min(area.width.saturating_sub(4));
    }

    fn handle_input(&mut self, c: char) -> OverlayEffect {
        self.input.push(c);
        OverlayEffect::None
    }

    fn handle_action(&mut self, action: &Action<MMAction>) -> OverlayEffect {
        match action {
            Action::Accept | Action::Custom(MMAction::Accept) => self.commit(),
            Action::DeleteChar => {
                self.input.pop();
                OverlayEffect::None
            }
            Action::DeleteWord => {
                let trimmed = self.input.trim_end();
                let last_space = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                self.input.truncate(last_space);
                OverlayEffect::None
            }
            Action::Cancel
            | Action::Quit(_)
            | Action::ToggleFocus
            | Action::FocusFilter
            | Action::FocusNav => OverlayEffect::Disable,
            _ => OverlayEffect::None,
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        const PROMPT: &str = " 󰑕 ";
        let inner_w = (area.width.saturating_sub(2 + PROMPT.len() as u16)) as usize;
        let visible = visible_suffix(&self.input, inner_w);
        let para = Paragraph::new(Line::from(vec![
            Span::styled(
                PROMPT,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(visible.to_string(), Style::default().fg(Color::White)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(
                    " 󰑕 ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        frame.render_widget(Clear, area);
        frame.render_widget(para, area);
    }

    fn area(&mut self, ui_area: &Rect) -> Result<Rect, [SizeHint; 2]> {
        let w = self
            .dialog_width
            .max(30)
            .min(ui_area.width.saturating_sub(2));
        Ok(Rect {
            x: ui_area.x + (ui_area.width.saturating_sub(w)) / 2,
            y: ui_area.y + ui_area.height.saturating_sub(3),
            width: w,
            height: 3,
        })
    }
}

pub struct UnzipOverlay {
    current: CurrentItem,
    src: String,
    dest: String,
    dialog_width: u16,
    tx: RenderSender<MMAction>,
}

impl UnzipOverlay {
    pub fn new(current: CurrentItem, tx: RenderSender<MMAction>) -> Self {
        Self {
            current,
            src: String::new(),
            dest: String::new(),
            dialog_width: 50,
            tx,
        }
    }

    fn commit(&mut self) -> OverlayEffect {
        let dest = self.dest.trim().to_string();
        if dest.is_empty() || self.src.is_empty() {
            return OverlayEffect::Disable;
        }
        if let Err(e) = fs::create_dir_all(&dest) {
            log::error!("fm unzip: create dir '{dest}': {e}");
            return OverlayEffect::Disable;
        }
        let src = &self.src;
        let result = extract_archive(src, &dest);
        if let Err(e) = result {
            log::error!("fm unzip '{src}' -> '{dest}': {e}");
        }
        let _ = self
            .tx
            .send(RenderCommand::Action(Action::Reload(String::new())));
        OverlayEffect::Disable
    }
}

impl Overlay for UnzipOverlay {
    type A = MMAction;

    fn on_enable(&mut self, area: &Rect) {
        self.src = self
            .current
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default();
        self.dest = archive_stem(&self.src).to_string();
        let for_initial = input_width(&self.dest);
        let half = area.width / 2;
        self.dialog_width = for_initial
            .max(half)
            .max(40)
            .min(area.width.saturating_sub(4));
    }

    fn handle_input(&mut self, c: char) -> OverlayEffect {
        self.dest.push(c);
        OverlayEffect::None
    }

    fn handle_action(&mut self, action: &Action<MMAction>) -> OverlayEffect {
        match action {
            Action::Accept | Action::Custom(MMAction::Accept) => self.commit(),
            Action::DeleteChar => {
                self.dest.pop();
                OverlayEffect::None
            }
            Action::DeleteWord => {
                let trimmed = self.dest.trim_end();
                let last_space = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                self.dest.truncate(last_space);
                OverlayEffect::None
            }
            Action::Cancel
            | Action::Quit(_)
            | Action::ToggleFocus
            | Action::FocusFilter
            | Action::FocusNav => OverlayEffect::Disable,
            _ => OverlayEffect::None,
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        const PROMPT: &str = " 󰋺 ";
        let inner_w = (area.width.saturating_sub(2 + PROMPT.len() as u16)) as usize;
        let visible_dest = visible_suffix(&self.dest, inner_w);
        let para = Paragraph::new(Line::from(vec![
            Span::styled(
                PROMPT,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(visible_dest.to_string(), Style::default().fg(Color::White)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(Span::styled(
                    format!(" 󰋺 {} ", self.src),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        frame.render_widget(Clear, area);
        frame.render_widget(para, area);
    }

    fn area(&mut self, ui_area: &Rect) -> Result<Rect, [SizeHint; 2]> {
        let w = self
            .dialog_width
            .max(30)
            .min(ui_area.width.saturating_sub(2));
        Ok(Rect {
            x: ui_area.x + (ui_area.width.saturating_sub(w)) / 2,
            y: ui_area.y + ui_area.height.saturating_sub(3),
            width: w,
            height: 3,
        })
    }
}

pub fn archive_stem(name: &str) -> &str {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    for suffix in &[".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4"] {
        if let Some(s) = base.strip_suffix(suffix) {
            return s;
        }
    }
    Path::new(base)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(base)
}

pub fn extract_archive(src: &str, dest_dir: &str) -> std::io::Result<()> {
    let lower = src.to_ascii_lowercase();
    let status = if lower.ends_with(".tar.gz")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".tar.zst")
        || lower.ends_with(".tar.lz4")
        || lower.ends_with(".tar")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tbz2")
    {
        Command::new("tar")
            .args(["-xf", src, "-C", dest_dir])
            .status()
    } else if lower.ends_with(".zip") {
        Command::new("unzip")
            .args(["-q", src, "-d", dest_dir])
            .status()
    } else if lower.ends_with(".7z") {
        Command::new("7z")
            .args(["x", src, &format!("-o{dest_dir}")])
            .status()
    } else if lower.ends_with(".rar") {
        Command::new("unrar")
            .args(["x", src, &format!("{dest_dir}/")])
            .status()
    } else {
        return Err(std::io::Error::other(format!(
            "unsupported archive format: {src}"
        )));
    }?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "extractor exited with status {status}"
        )))
    }
}

pub fn create_archive(archive_name: &str, paths: &[String]) -> std::io::Result<()> {
    use std::process::Command;
    let lower = archive_name.to_ascii_lowercase();
    let is_tar = lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tbz2")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".tar.zst")
        || lower.ends_with(".tar");

    let final_name = if is_tar || lower.ends_with(".zip") || lower.ends_with(".7z") {
        archive_name.to_string()
    } else {
        format!("{}.zip", archive_name)
    };

    let lower_final = final_name.to_ascii_lowercase();
    let status = if lower_final.ends_with(".tar.gz") || lower_final.ends_with(".tgz") {
        Command::new("tar")
            .arg("-czf")
            .arg(&final_name)
            .args(paths)
            .status()
    } else if lower_final.ends_with(".tar.bz2") || lower_final.ends_with(".tbz2") {
        Command::new("tar")
            .arg("-cjf")
            .arg(&final_name)
            .args(paths)
            .status()
    } else if lower_final.ends_with(".tar.xz") {
        Command::new("tar")
            .arg("-cJf")
            .arg(&final_name)
            .args(paths)
            .status()
    } else if lower_final.ends_with(".tar") {
        Command::new("tar")
            .arg("-cf")
            .arg(&final_name)
            .args(paths)
            .status()
    } else if lower_final.ends_with(".zip") {
        Command::new("zip")
            .arg("-r")
            .arg(&final_name)
            .args(paths)
            .status()
    } else if lower_final.ends_with(".7z") {
        Command::new("7z")
            .arg("a")
            .arg(&final_name)
            .args(paths)
            .status()
    } else {
        return Err(std::io::Error::other(format!(
            "unsupported archive creation format: {final_name}"
        )));
    }?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "archiver exited with status {status}"
        )))
    }
}

pub fn copy_into(src: &Path, dest_dir: &Path) -> std::io::Result<PathBuf> {
    let file_name = src
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;

    let dest = unique_dest(dest_dir, Path::new(file_name));

    if src.is_dir() {
        copy_dir_all(src, &dest)?;
    } else {
        fs::copy(src, &dest)?;
    }
    Ok(dest)
}

pub fn move_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    match fs::rename(src, dst) {
        Ok(_) => Ok(()),
        Err(_) => {
            if src.is_dir() {
                copy_dir_all(src, dst)?;
                fs::remove_dir_all(src)
            } else {
                fs::copy(src, dst)?;
                fs::remove_file(src)
            }
        }
    }
}

pub fn move_into(src: &Path, dest_dir: &Path) -> std::io::Result<PathBuf> {
    let file_name = src
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;

    let dest = unique_dest(dest_dir, Path::new(file_name));
    move_path(src, &dest)?;
    Ok(dest)
}

fn unique_dest(dir: &Path, name: &Path) -> PathBuf {
    let stem = name.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = name
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    let base = dir.join(name);
    if !base.exists() {
        return base;
    }
    let mut n = 1u32;
    loop {
        let candidate = dir.join(format!("{stem}_{n}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

fn input_width(s: &str) -> u16 {
    (s.len() as u16 + 20).max(30)
}

fn visible_suffix(s: &str, max_cols: usize) -> &str {
    if s.len() <= max_cols {
        return s;
    }
    let byte_start = s.len() - max_cols;
    let start = (byte_start..=s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(s.len());
    &s[start..]
}
