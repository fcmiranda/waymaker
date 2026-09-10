use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::fs;
use std::path::Path;

use crate::ui::results::icon_for_name;

/// Options for directory tree rendering matching `eza` flags.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TreeOptions {
    pub max_depth: usize,
    pub show_icons: bool,
    pub git_ignore: bool,
    pub show_hidden: bool,
}

impl Default for TreeOptions {
    fn default() -> Self {
        Self {
            max_depth: 2,
            show_icons: true,
            git_ignore: true,
            show_hidden: false,
        }
    }
}

/// Render a directory tree as Ratatui `Text<'static>` in pure Rust with sub-millisecond execution time.
pub fn render_dir_tree(dir: &Path, opts: &TreeOptions) -> Text<'static> {
    let mut lines = Vec::new();
    let root_str = dir.to_string_lossy();
    let (icon, icon_color) = icon_for_name(&root_str);

    let mut root_spans = vec![];
    if opts.show_icons {
        root_spans.push(Span::styled(
            format!("{icon} "),
            Style::default().fg(icon_color),
        ));
    }
    root_spans.push(Span::styled(
        root_str.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::from(root_spans));

    build_tree(dir, "", 1, opts, &mut lines);
    Text::from(lines)
}

/// Render a directory tree as ANSI colored string for terminal stdout.
pub fn render_dir_tree_ansi(dir: &Path, opts: &TreeOptions) -> String {
    let text = render_dir_tree(dir, opts);
    super::text::text_to_ansi(&text)
}

fn build_tree(
    dir: &Path,
    prefix: &str,
    depth: usize,
    opts: &TreeOptions,
    lines: &mut Vec<Line<'static>>,
) {
    if depth > opts.max_depth {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut valid_entries: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();

            if !opts.show_hidden && name_str.starts_with('.') && name_str != "." && name_str != ".."
            {
                return false;
            }

            if opts.git_ignore {
                match name_str.as_ref() {
                    ".git" | "node_modules" | "target" | ".cache" | "dist" | "build"
                    | "__pycache__" | ".venv" | "venv" | ".cargo" => {
                        return false;
                    }
                    _ => {}
                }
            }
            true
        })
        .collect();

    valid_entries.sort_by_key(|e| {
        (
            e.file_type().map(|t| !t.is_dir()).unwrap_or(true),
            e.file_name(),
        )
    });

    let total = valid_entries.len();
    for (i, entry) in valid_entries.into_iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);

        let mut spans = vec![Span::styled(
            format!("{prefix}{connector}"),
            Style::default().fg(Color::DarkGray),
        )];

        let name_for_icon = if is_dir {
            format!("{name_str}/")
        } else {
            name_str.to_string()
        };
        let (icon, icon_color) = icon_for_name(&name_for_icon);

        if opts.show_icons {
            spans.push(Span::styled(
                format!("{icon} "),
                Style::default().fg(icon_color),
            ));
        }

        let name_style = if is_dir {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else if is_symlink {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Reset)
        };

        spans.push(Span::styled(name_str.to_string(), name_style));

        if is_symlink {
            if let Ok(target) = fs::read_link(&path) {
                spans.push(Span::styled(
                    format!(" -> {}", target.display()),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        } else if !is_dir {
            if let Ok(meta) = entry.metadata() {
                let size = format_size(meta.len());
                spans.push(Span::styled(
                    format!(" ({size})"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        lines.push(Line::from(spans));

        if is_dir && depth < opts.max_depth {
            let next_prefix = format!("{prefix}{child_prefix}");
            build_tree(&path, &next_prefix, depth + 1, opts, lines);
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_dir_tree_basic() {
        let temp_dir = std::env::temp_dir().join("mm_tree_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("subfolder")).unwrap();
        fs::write(temp_dir.join("file1.rs"), "fn main() {}").unwrap();
        fs::write(temp_dir.join("subfolder/file2.txt"), "hello").unwrap();

        let opts = TreeOptions::default();
        let text = render_dir_tree(&temp_dir, &opts);
        assert!(!text.lines.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
