use ratatui::style::Color;
use std::collections::HashSet;
use unicode_width::UnicodeWidthStr;

use crate::{config::StyleSetting, ui::results::ResultsUI};

thread_local! {
    static SYMLINK_CACHE: std::cell::RefCell<rustc_hash::FxHashMap<String, Option<String>>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    static ICON_CACHE: std::cell::RefCell<rustc_hash::FxHashMap<String, (char, Color)>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

impl ResultsUI {
    #[inline]
    pub(super) fn is_path_in_set(
        set: &HashSet<String>,
        col0_name: &str,
        cwd: &std::path::Path,
    ) -> bool {
        if set.is_empty() || col0_name.is_empty() {
            return false;
        }
        let trimmed = col0_name
            .trim()
            .trim_end_matches('/')
            .trim_end_matches('\\');
        if set.contains(col0_name) || set.contains(trimmed) {
            return true;
        }
        let with_slash = format!("{trimmed}/");
        if set.contains(&with_slash) {
            return true;
        }

        let path = std::path::Path::new(trimmed);
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        let abs_str = abs_path.to_string_lossy();
        let abs_trimmed = abs_str.trim_end_matches('/').trim_end_matches('\\');
        let abs_slash = format!("{abs_trimmed}/");

        if set.contains(abs_trimmed) || set.contains(&abs_slash) || set.contains(abs_str.as_ref()) {
            return true;
        }

        for target in set {
            let target_trimmed = target.trim().trim_end_matches('/').trim_end_matches('\\');
            if trimmed == target_trimmed
                || abs_trimmed == target_trimmed
                || target_trimmed.ends_with(&format!("/{}", trimmed))
                || abs_trimmed.ends_with(&format!("/{}", target_trimmed))
            {
                return true;
            }
            let target_path = std::path::Path::new(target_trimmed);
            let abs_target = if target_path.is_absolute() {
                target_path.to_path_buf()
            } else {
                cwd.join(target_path)
            };
            let abs_target_str = abs_target.to_string_lossy();
            let abs_target_trimmed = abs_target_str.trim_end_matches('/').trim_end_matches('\\');
            if abs_trimmed == abs_target_trimmed
                || abs_target_trimmed.ends_with(&format!("/{}", trimmed))
                || abs_trimmed.ends_with(&format!("/{}", target_trimmed))
            {
                return true;
            }
        }
        false
    }

    /// Return the correct inactive prefix style for a given row.
    ///
    /// Priority: cut > yank > pin > selected > default.
    #[inline]
    pub(super) fn inactive_prefix_style(
        &self,
        col0_name: &str,
        is_selected: bool,
        is_spinner: bool,
        cwd: &std::path::Path,
    ) -> StyleSetting {
        if is_spinner {
            return self.config.spinner_style;
        }
        if let Some(op) = self.get_flash_op(col0_name, cwd) {
            return match op {
                crate::ui::results::FlashOp::Cut => StyleSetting {
                    fg: Some(ratatui::style::Color::Red),
                    bg: None,
                    modifier: ratatui::style::Modifier::BOLD,
                },
                crate::ui::results::FlashOp::Copy => StyleSetting {
                    fg: Some(ratatui::style::Color::Yellow),
                    bg: None,
                    modifier: ratatui::style::Modifier::BOLD,
                },
            };
        }
        if self.cut_paths.is_empty()
            && self.yank_paths.is_empty()
            && self.pin_paths.is_empty()
            && self.mode_index != 2
        {
            return if is_selected {
                self.config.selected_prefix_style
            } else {
                self.config.prefix_inactive_style
            };
        }

        if Self::is_path_in_set(&self.cut_paths, col0_name, cwd) {
            self.config.cut_prefix_style
        } else if Self::is_path_in_set(&self.yank_paths, col0_name, cwd) {
            self.config.yank_prefix_style
        } else if Self::is_path_in_set(&self.pin_paths, col0_name, cwd) || self.mode_index == 2 {
            StyleSetting {
                fg: Some(bookmark_color(&self.config, col0_name)),
                bg: None,
                modifier: ratatui::style::Modifier::BOLD,
            }
        } else if is_selected {
            self.config.selected_prefix_style
        } else {
            self.config.prefix_inactive_style
        }
    }

    #[inline]
    pub(super) fn active_prefix_style(
        &self,
        col0_name: &str,
        is_selected: bool,
        is_spinner: bool,
        cwd: &std::path::Path,
    ) -> StyleSetting {
        if is_spinner {
            return self.config.spinner_style;
        }
        if let Some(op) = self.get_flash_op(col0_name, cwd) {
            return match op {
                crate::ui::results::FlashOp::Cut => StyleSetting {
                    fg: Some(ratatui::style::Color::Red),
                    bg: None,
                    modifier: ratatui::style::Modifier::BOLD,
                },
                crate::ui::results::FlashOp::Copy => StyleSetting {
                    fg: Some(ratatui::style::Color::Yellow),
                    bg: None,
                    modifier: ratatui::style::Modifier::BOLD,
                },
            };
        }
        if self.cut_paths.is_empty()
            && self.yank_paths.is_empty()
            && self.pin_paths.is_empty()
            && self.mode_index != 2
        {
            return if is_selected {
                self.config.selected_prefix_style
            } else {
                self.config.prefix_style
            };
        }

        if Self::is_path_in_set(&self.cut_paths, col0_name, cwd) {
            self.config.cut_prefix_style
        } else if Self::is_path_in_set(&self.yank_paths, col0_name, cwd) {
            self.config.yank_prefix_style
        } else if Self::is_path_in_set(&self.pin_paths, col0_name, cwd) || self.mode_index == 2 {
            StyleSetting {
                fg: Some(bookmark_color(&self.config, col0_name)),
                bg: None,
                modifier: ratatui::style::Modifier::BOLD,
            }
        } else if is_selected {
            self.config.selected_prefix_style
        } else {
            self.config.prefix_style
        }
    }
}

/// Append a symlink-target annotation to the **first line** of `col`.
///
/// Reads the link target with `std::fs::read_link`. If the path is not a
/// symlink (or the read fails) the function is a no-op. The annotation is
/// rendered as `" \u{f061} <target>"` using `style`, truncated with `…` if
/// it would overflow `max_width`.
pub(super) fn maybe_append_symlink_target(
    col: &mut ratatui::text::Text<'_>,
    name: &str,
    style: ratatui::style::Style,
    max_width: u16,
) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }

    let target_opt = SYMLINK_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if map.len() > 1024 {
            map.clear();
        }
        map.entry(trimmed.to_string())
            .or_insert_with(|| {
                std::path::Path::new(trimmed)
                    .read_link()
                    .ok()
                    .map(|t| t.to_string_lossy().into_owned())
            })
            .clone()
    });

    let Some(target_str) = target_opt else {
        return;
    };

    let arrow = " \u{f061} ";
    let arrow_width = arrow.width();

    // Measure how much width the first line already occupies.
    let used: usize = col
        .lines
        .first()
        .map(|l| l.spans.iter().map(|s| s.content.width()).sum())
        .unwrap_or(0);

    let remaining = (max_width as usize).saturating_sub(used);

    // Need at least space for the arrow + 2 chars to show anything useful safely.
    if remaining < arrow_width + 2 {
        return;
    }

    let budget = remaining.saturating_sub(arrow_width + 1);
    let annotation = if target_str.width() <= budget {
        format!("{arrow}{target_str}")
    } else {
        // Truncate target to budget - 1 chars + "…"
        let mut truncated = String::new();
        let mut w = 0;
        for g in unicode_segmentation::UnicodeSegmentation::graphemes(target_str.as_str(), true) {
            let gw = g.width();
            if w + gw + 1 > budget {
                break;
            }
            truncated.push_str(g);
            w += gw;
        }
        format!("{arrow}{truncated}…")
    };

    let span = ratatui::text::Span::styled(annotation, style);
    if let Some(line) = col.lines.first_mut() {
        line.spans.push(span);
    }
}

/// Extract the plain-text content of the first line of a `Text` cell (before
/// any prefix span has been inserted). Used to determine which file-type icon
/// to display.
pub(super) fn extract_col0_name(col: &ratatui::text::Text<'_>) -> String {
    col.lines
        .first()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .unwrap_or_default()
}

pub(super) fn invert_color(c: Color) -> Color {
    match c {
        Color::Reset => Color::Reset,
        Color::Black => Color::White,
        Color::White => Color::Black,
        Color::Red => Color::Cyan,
        Color::Green => Color::Magenta,
        Color::Yellow => Color::Blue,
        Color::Blue => Color::Yellow,
        Color::Magenta => Color::Green,
        Color::Cyan => Color::Red,
        Color::Gray => Color::DarkGray,
        Color::DarkGray => Color::Gray,
        Color::LightRed => Color::LightCyan,
        Color::LightGreen => Color::LightMagenta,
        Color::LightYellow => Color::LightBlue,
        Color::LightBlue => Color::LightYellow,
        Color::LightMagenta => Color::LightGreen,
        Color::LightCyan => Color::LightRed,
        Color::Rgb(r, g, b) => Color::Rgb(255 - r, 255 - g, 255 - b),
        Color::Indexed(i) => Color::Indexed(255 - i),
    }
}

/// Insert a Nerd-Font icon span after the prefix in every line of `col`.
/// Callers must ensure `prefix_span` has already been called.
/// When `is_current_row` is true:
/// - `current_icon_style` takes precedence if specified.
/// - `invert_current` inverts the icon's natural color (e.g. Blue <-> Yellow).
/// - `uncolor_current` drops the color so it inherits line text highlight.
/// - Otherwise, natural icon color is preserved.
pub(super) fn insert_icon_span(
    col: &mut ratatui::text::Text<'_>,
    name: &str,
    has_nav_bar: bool,
    is_current_row: bool,
    uncolor_current: bool,
    invert_current: bool,
    current_icon_style: StyleSetting,
    is_pinned: bool,
    is_yanked: bool,
    is_cut: bool,
    mode_index: usize,
    results_config: &crate::config::ResultsConfig,
    flash_op: Option<crate::ui::results::FlashOp>,
) {
    let (icon_str, color): (std::borrow::Cow<'_, str>, Color) = if let Some(op) = flash_op {
        match op {
            crate::ui::results::FlashOp::Cut => ("󰆐".into(), Color::Red),
            crate::ui::results::FlashOp::Copy => ("󰆏".into(), Color::Yellow),
        }
    } else if is_cut {
        let (ch, _) = icon_for_name(name);
        (ch.to_string().into(), Color::Red)
    } else if is_yanked {
        let (ch, _) = icon_for_name(name);
        (ch.to_string().into(), Color::Yellow)
    } else if is_pinned || mode_index == 2 {
        let trimmed = name.trim();
        let is_dir = trimmed.ends_with('/')
            || trimmed.ends_with('\\')
            || std::path::Path::new(trimmed).is_dir();
        let icon = if is_dir {
            results_config
                .bookmark_folder_icon
                .as_deref()
                .or(results_config.bookmark_icon.as_deref())
                .unwrap_or("󰮟")
        } else {
            results_config
                .bookmark_file_icon
                .as_deref()
                .or(results_config.bookmark_icon.as_deref())
                .unwrap_or("󱀻")
        };
        let color = bookmark_color(results_config, name);
        (icon.into(), color)
    } else if mode_index == 1 {
        let trimmed = name.trim();
        let is_dir = trimmed.ends_with('/')
            || trimmed.ends_with('\\')
            || std::path::Path::new(trimmed).is_dir();
        if is_dir {
            let icon = results_config
                .frecency_folder_icon
                .as_deref()
                .unwrap_or("󰪻");
            let color = results_config
                .frecency_folder_icon_style
                .fg
                .unwrap_or(Color::Blue);
            (icon.into(), color)
        } else {
            let icon = results_config.frecency_icon.as_deref().unwrap_or("󱋢");
            let color = results_config.frecency_icon_style.fg.unwrap_or(Color::Blue);
            (icon.into(), color)
        }
    } else {
        let (ch, c) = icon_for_name(name);
        (ch.to_string().into(), c)
    };
    let style = if flash_op.is_some() || is_cut || is_yanked {
        ratatui::style::Style::default()
            .fg(color)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else if is_current_row {
        if is_pinned || mode_index == 2 {
            ratatui::style::Style::default().fg(color)
        } else if current_icon_style.fg.is_some()
            || current_icon_style.bg.is_some()
            || !current_icon_style.modifier.is_empty()
        {
            ratatui::style::Style::from(current_icon_style)
        } else if invert_current {
            ratatui::style::Style::default().fg(invert_color(color))
        } else if uncolor_current {
            ratatui::style::Style::default()
        } else {
            ratatui::style::Style::default().fg(color)
        }
    } else {
        ratatui::style::Style::default().fg(color)
    };
    let icon_span = ratatui::text::Span::styled(icon_str.into_owned(), style);
    let index = if has_nav_bar { 2 } else { 1 };
    for line in col.lines.iter_mut() {
        let at = index.min(line.spans.len());
        line.spans.insert(at, ratatui::text::Span::raw(" "));
        line.spans.insert(at + 1, icon_span.clone());
        line.spans.insert(at + 2, ratatui::text::Span::raw(" "));
    }
}

pub(crate) fn bookmark_color(results_config: &crate::config::ResultsConfig, name: &str) -> Color {
    let trimmed = name.trim();
    let is_dir =
        trimmed.ends_with('/') || trimmed.ends_with('\\') || std::path::Path::new(trimmed).is_dir();
    if is_dir {
        results_config
            .bookmark_folder_icon_style
            .fg
            .or(results_config.bookmark_icon_style.fg)
            .unwrap_or(Color::Yellow)
    } else {
        results_config
            .bookmark_file_icon_style
            .fg
            .or(results_config.bookmark_icon_style.fg)
            .unwrap_or(Color::Yellow)
    }
}

pub(crate) fn apply_bookmark_text_style(t: &mut ratatui::text::Text<'_>, color: Color) {
    for line in t.lines.iter_mut() {
        for span in line.spans.iter_mut() {
            if span.style.fg.is_none() {
                span.style = span.style.fg(color);
            }
        }
    }
}

/// Return the Nerd-Font glyph and colour for a given file/directory name.
///
/// Lookup order: directory → symlink → known basename → file extension →
/// generic file fallback.
pub fn icon_for_name(name: &str) -> (char, Color) {
    let trimmed = name.trim();
    if trimmed.ends_with('/') || trimmed.ends_with('\\') {
        return ('\u{f115}', Color::Blue); // nf-fa-folder_open
    }

    ICON_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some(&res) = map.get(trimmed) {
            return res;
        }
        if map.len() > 2048 {
            map.clear();
        }

        let res = compute_icon_for_name(trimmed);
        map.insert(trimmed.to_string(), res);
        res
    })
}

fn compute_icon_for_name(trimmed: &str) -> (char, Color) {
    use std::path::Path;
    let path = Path::new(trimmed);

    // Directory
    if std::fs::metadata(path).is_ok_and(|m| m.is_dir()) {
        return ('\u{f115}', Color::Blue); // nf-fa-folder_open
    }
    // Symlink
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return ('\u{f482}', Color::Cyan); // nf-mdi-link
    }

    let basename = path.file_name().and_then(|n| n.to_str()).unwrap_or(trimmed);

    match basename.to_lowercase().as_str() {
        "cargo.toml" | "cargo.lock" => return ('\u{e7a8}', Color::Red),
        "package.json" | "package-lock.json" | "yarn.lock" => return ('\u{e74e}', Color::Green),
        "makefile" | "gnumakefile" => return ('\u{e779}', Color::Yellow),
        "dockerfile" => return ('\u{e7b0}', Color::Cyan),
        ".gitignore" | ".gitmodules" | ".gitattributes" => return ('\u{e702}', Color::Red),
        "readme.md" | "readme.txt" | "readme" => return ('\u{e73e}', Color::Blue),
        "license" | "license.md" | "license.txt" => return ('\u{f02d}', Color::Yellow),
        _ => {}
    }

    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => ('\u{e7a8}', Color::Red),
        "toml" => ('\u{e6b2}', Color::Gray),
        "json" => ('\u{e60b}', Color::Yellow),
        "yaml" | "yml" => ('\u{e8eb}', Color::Yellow),
        "js" | "mjs" | "cjs" => ('\u{e74e}', Color::Yellow),
        "ts" | "mts" | "cts" => ('\u{e628}', Color::Blue),
        "jsx" | "tsx" => ('\u{e7ba}', Color::Cyan),
        "py" | "pyw" => ('\u{e73c}', Color::Yellow),
        "html" | "htm" => ('\u{e736}', Color::Red),
        "css" | "scss" | "sass" | "less" => ('\u{e749}', Color::Cyan),
        "sh" | "bash" | "zsh" | "fish" | "ksh" => ('\u{f489}', Color::Green),
        "md" | "mdx" | "markdown" => ('\u{e73e}', Color::Blue),
        "txt" | "text" => ('\u{f15c}', Color::Gray),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" => {
            ('\u{f1c5}', Color::Magenta)
        }
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" => ('\u{f03d}', Color::Magenta),
        "mp3" | "flac" | "ogg" | "wav" | "aac" | "opus" => ('\u{f001}', Color::Magenta),
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" => ('\u{f410}', Color::Yellow),
        "pdf" => ('\u{f1c1}', Color::Red),
        "c" | "h" => ('\u{e61e}', Color::Blue),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => ('\u{e61d}', Color::Blue),
        "go" => ('\u{e724}', Color::Cyan),
        "java" | "class" | "jar" => ('\u{e738}', Color::Red),
        "rb" => ('\u{e739}', Color::Red),
        "php" => ('\u{e73d}', Color::Magenta),
        "lua" => ('\u{e620}', Color::Blue),
        "vim" | "nvim" => ('\u{e7c5}', Color::Green),
        "lock" => ('\u{f023}', Color::Yellow),
        "env" | "envrc" => ('\u{f462}', Color::Yellow),
        "xml" => ('\u{e619}', Color::Yellow),
        "sql" => ('\u{e706}', Color::Gray),
        "nix" => ('\u{f313}', Color::Cyan),
        "swift" => ('\u{e755}', Color::Red),
        "kt" | "kts" => ('\u{e634}', Color::Magenta),
        "cs" => ('\u{f81a}', Color::Magenta),
        "ex" | "exs" => ('\u{e62d}', Color::Magenta),
        "hs" | "lhs" => ('\u{e61f}', Color::Magenta),
        "ml" | "mli" => ('\u{e67a}', Color::Yellow),
        "r" | "rmd" => ('\u{f25d}', Color::Blue),
        "tf" | "tfvars" => ('\u{e20f}', Color::Magenta),
        _ => ('\u{f15b}', Color::Gray), // nf-fa-file
    }
}
