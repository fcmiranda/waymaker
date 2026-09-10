use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::fs;
use std::path::Path;

use super::text::text_to_ansi;

/// Options for Mermaid diagram rendering.
#[derive(Debug, Clone, Default)]
pub struct MermaidOptions {
    /// Column budget for compaction (None = unlimited/natural width).
    pub max_width: Option<usize>,
    /// Whether to force ASCII-only characters (no Unicode box-drawing).
    pub ascii: bool,
    /// Whether to wrap the diagram in a decorative outer box.
    pub show_box: bool,
    /// Optional title to show in the diagram header.
    pub title: Option<String>,
}

/// Render a Mermaid diagram source string to Ratatui `Text<'static>`.
pub fn render_mermaid(src: &str, opts: &MermaidOptions) -> Text<'static> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Text::default();
    }

    let render_result = if opts.ascii {
        mermaid_text::render_ascii_with_width(trimmed, opts.max_width)
    } else {
        mermaid_text::render_with_width(trimmed, opts.max_width)
    };

    match render_result {
        Ok(rendered) => style_mermaid_output(&rendered, opts),
        Err(err) => render_mermaid_error(trimmed, &err.to_string(), opts),
    }
}

/// Render a Mermaid diagram to an ANSI colored string for terminal stdout.
pub fn render_mermaid_ansi(src: &str, opts: &MermaidOptions) -> String {
    let text = render_mermaid(src, opts);
    text_to_ansi(&text)
}

/// Read a Mermaid file and render it as Ratatui `Text<'static>`.
pub fn render_mermaid_file(path: &Path, opts: &MermaidOptions) -> anyhow::Result<Text<'static>> {
    let content = fs::read_to_string(path)?;
    Ok(render_mermaid(&content, opts))
}

/// Read a Mermaid file and render it as an ANSI colored string for terminal stdout.
pub fn render_mermaid_file_ansi(path: &Path, opts: &MermaidOptions) -> anyhow::Result<String> {
    let content = fs::read_to_string(path)?;
    Ok(render_mermaid_ansi(&content, opts))
}

/// Style the plain-text diagram output with theme colors for box borders, connectors, and labels.
fn style_mermaid_output(rendered: &str, opts: &MermaidOptions) -> Text<'static> {
    let raw_lines: Vec<&str> = rendered.lines().collect();
    if raw_lines.is_empty() {
        return Text::default();
    }

    let mut lines = Vec::new();

    let border_style = Style::default().fg(Color::Cyan);
    let text_style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);

    if opts.show_box {
        let title_text = opts.title.as_deref().unwrap_or("Mermaid Diagram");
        let header_span = Span::styled(
            format!("╭─ [{title_text}] ──────────────────────────────────────────╮"),
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        );
        lines.push(Line::from(header_span));
    }

    for raw_line in raw_lines {
        let mut spans = Vec::new();
        let mut current_text = String::new();
        let mut current_is_box = false;

        for ch in raw_line.chars() {
            let is_box = is_diagram_glyph(ch);
            if spans.is_empty() && current_text.is_empty() {
                current_is_box = is_box;
                current_text.push(ch);
            } else if is_box == current_is_box {
                current_text.push(ch);
            } else {
                let style = if current_is_box {
                    border_style
                } else {
                    text_style
                };
                spans.push(Span::styled(std::mem::take(&mut current_text), style));
                current_is_box = is_box;
                current_text.push(ch);
            }
        }

        if !current_text.is_empty() {
            let style = if current_is_box {
                border_style
            } else {
                text_style
            };
            spans.push(Span::styled(current_text, style));
        }

        if opts.show_box {
            let mut boxed_spans = vec![Span::styled("│ ", Style::default().fg(Color::Blue))];
            boxed_spans.extend(spans);
            lines.push(Line::from(boxed_spans));
        } else {
            lines.push(Line::from(spans));
        }
    }

    if opts.show_box {
        let footer_span = Span::styled(
            "╰────────────────────────────────────────────────────────────╯",
            Style::default().fg(Color::Blue),
        );
        lines.push(Line::from(footer_span));
    }

    Text::from(lines)
}

/// Check if a character is a diagram border, connector, or edge glyph.
fn is_diagram_glyph(ch: char) -> bool {
    matches!(
        ch,
        '─' | '│'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
            | '╭'
            | '╮'
            | '╯'
            | '╰'
            | '═'
            | '║'
            | '▲'
            | '▼'
            | '◄'
            | '►'
            | '▸'
            | '▾'
            | '▴'
            | '◀'
            | '▶'
            | '╱'
            | '╲'
            | '┄'
            | '┆'
            | '┈'
            | '┊'
            | '╴'
            | '╵'
            | '╶'
            | '╷'
    )
}

/// Render a diagram syntax error gracefully with a warning badge and the raw source.
fn render_mermaid_error(src: &str, error_msg: &str, _opts: &MermaidOptions) -> Text<'static> {
    let mut lines = Vec::new();
    let err_border = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);

    lines.push(Line::from(Span::styled(
        "╭─ [Mermaid Syntax Warning] ──────────────────────────╮",
        err_border,
    )));
    lines.push(Line::from(vec![
        Span::styled("│ ", err_border),
        Span::styled(
            format!("Error: {error_msg}"),
            Style::default().fg(Color::LightRed),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "├─ Source ────────────────────────────────────────────┤",
        err_border,
    )));

    for src_line in src.lines() {
        lines.push(Line::from(vec![
            Span::styled("│ ", err_border),
            Span::styled(
                src_line.to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    lines.push(Line::from(Span::styled(
        "╰─────────────────────────────────────────────────────╯",
        err_border,
    )));

    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_flowchart_basic() {
        let src = "graph TD\n    A[Start] --> B[End]";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
        assert!(!text.lines.is_empty());
        let joined = text
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Start"));
        assert!(joined.contains("End"));
    }

    #[test]
    fn test_render_ascii_mode() {
        let src = "graph LR\n    A[Left] --> B[Right]";
        let mut opts = MermaidOptions::default();
        opts.ascii = true;
        let text = render_mermaid(src, &opts);
        let joined = text
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Left"));
        assert!(joined.contains("Right"));
        assert!(joined.is_ascii());
    }

    #[test]
    fn test_render_width_constrained() {
        let src = "graph LR\n    A[Start] --> B[Process] --> C[Done]";
        let mut opts = MermaidOptions::default();
        opts.max_width = Some(40);
        let text = render_mermaid(src, &opts);
        assert!(!text.lines.is_empty());
    }

    #[test]
    fn test_render_error_fallback() {
        let src = "this is not valid mermaid @#$*&";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
        let joined = text
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Mermaid Syntax Warning"));
        assert!(joined.contains("this is not valid mermaid"));
    }

    #[test]
    fn test_render_empty() {
        let src = "   \n\t  ";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
        assert!(text.lines.is_empty());
    }

    #[test]
    fn test_render_ansi() {
        let src = "graph TD\n    A[NodeA] --> B[NodeB]";
        let opts = MermaidOptions::default();
        let ansi = render_mermaid_ansi(src, &opts);
        assert!(ansi.contains("NodeA"));
        assert!(ansi.contains("NodeB"));
    }
}
