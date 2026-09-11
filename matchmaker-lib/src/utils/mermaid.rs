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

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHasher};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

/// Shared system font database for fast SVG text rasterization.
static FONT_DB: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    Arc::new(db)
});

/// In-memory LRU cache for rendered diagram bitmaps, keyed by (FxHash(source), scale_key).
static DIAGRAM_CACHE: LazyLock<Mutex<IndexMap<(u64, u32), image::DynamicImage, FxBuildHasher>>> =
    LazyLock::new(|| Mutex::new(IndexMap::with_capacity_and_hasher(64, FxBuildHasher)));

/// Render a Mermaid diagram source string to a `DynamicImage` using the graphics pipeline.
///
/// Uses `mermaid-rs-renderer` to produce SVG, then rasterizes with `resvg`/`usvg`/`tiny-skia`.
/// The `scale` parameter multiplies the SVG's natural pixel dimensions, enabling zoom.
/// Results are cached in an in-memory LRU cache to guarantee sub-millisecond redraws.
/// Returns `None` if rendering fails (caller should fall back to text rendering).
pub fn render_mermaid_to_image(src: &str, scale: f32) -> Option<image::DynamicImage> {
    use mermaid_rs_renderer::{RenderOptions, render_with_options};

    let trimmed = src.trim();
    if trimmed.is_empty() {
        return None;
    }

    let scale_val = scale.max(0.1);
    let scale_key = (scale_val * 100.0) as u32;

    let mut hasher = FxHasher::default();
    trimmed.hash(&mut hasher);
    let hash = hasher.finish();

    // Check LRU cache first
    if let Ok(mut cache) = DIAGRAM_CACHE.lock() {
        if let Some((_, img)) = cache.shift_remove_entry(&(hash, scale_key)) {
            let cloned = img.clone();
            cache.insert((hash, scale_key), img);
            return Some(cloned);
        }
    }

    // Stage 1: Mermaid → SVG
    let svg = render_with_options(trimmed, RenderOptions::default()).ok()?;

    // Stage 2: SVG → rasterized image via usvg + resvg + tiny-skia
    let img = render_svg_to_image(&svg, scale_val)?;

    // Store in LRU cache (evict oldest if over capacity)
    if let Ok(mut cache) = DIAGRAM_CACHE.lock() {
        if cache.len() >= 64 {
            cache.shift_remove_index(0);
        }
        cache.insert((hash, scale_key), img.clone());
    }

    Some(img)
}

/// Rasterize an SVG string to a `DynamicImage` at the given scale factor.
///
/// Uses the same font / pixmap pipeline as `mermaid-rs-renderer`'s `write_output_png`,
/// utilizing the shared `FONT_DB` to avoid reloading system fonts on each render.
pub fn render_svg_to_image(svg: &str, scale: f32) -> Option<image::DynamicImage> {
    let opt = usvg::Options {
        fontdb: FONT_DB.clone(),
        ..Default::default()
    };

    let tree = usvg::Tree::from_str(svg, &opt).ok()?;
    let natural = tree.size().to_int_size();

    let w = ((natural.width() as f32) * scale.max(0.1)) as u32;
    let h = ((natural.height() as f32) * scale.max(0.1)) as u32;

    if w == 0 || h == 0 {
        return None;
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;

    // White background (Mermaid diagrams are designed for light backgrounds)
    pixmap.fill(resvg::tiny_skia::Color::WHITE);

    let transform = resvg::tiny_skia::Transform::from_scale(
        w as f32 / natural.width() as f32,
        h as f32 / natural.height() as f32,
    );

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Convert RGBA pixmap to image::DynamicImage
    let rgba_data = pixmap.data().to_vec();
    let img = image::RgbaImage::from_raw(w, h, rgba_data)?;
    Some(image::DynamicImage::ImageRgba8(img))
}

/// Read a `.mmd`/`.mermaid` file and render it as a `DynamicImage`.
/// Falls back to `None` if graphics rendering fails.
pub fn render_mermaid_file_to_image(path: &Path, scale: f32) -> Option<image::DynamicImage> {
    let content = fs::read_to_string(path).ok()?;
    render_mermaid_to_image(&content, scale)
}

use unicode_width::UnicodeWidthStr;

/// Style the plain-text diagram output with theme colors for box borders, connectors, and labels.
fn style_mermaid_output(rendered: &str, opts: &MermaidOptions) -> Text<'static> {
    let raw_lines: Vec<&str> = rendered.lines().collect();
    if raw_lines.is_empty() {
        return Text::default();
    }

    let mut lines = Vec::new();

    let border_style = Style::default().fg(Color::Cyan);
    let text_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    let content_width = raw_lines
        .iter()
        .map(|l| UnicodeWidthStr::width(*l))
        .max()
        .unwrap_or(0);

    let title_str = opts.title.as_deref().unwrap_or("Mermaid Diagram");
    let title_w = UnicodeWidthStr::width(title_str);

    let inner_width = content_width.max(title_w + 4);
    let box_width = if let Some(mw) = opts.max_width {
        inner_width.min(mw.saturating_sub(4).max(inner_width.min(20)))
    } else {
        inner_width
    };

    if opts.show_box {
        if opts.ascii {
            let mut top = format!("+-[ {title_str} ]");
            let remaining = (box_width + 4).saturating_sub(top.len() + 1);
            top.push_str(&"-".repeat(remaining));
            top.push('+');
            lines.push(Line::from(Span::styled(top, border_style)));
        } else {
            let prefix = format!("╭─ [{title_str}] ");
            let p_len = UnicodeWidthStr::width(prefix.as_str());
            let fill_count = (box_width + 4).saturating_sub(p_len + 1);
            let mut top = prefix;
            top.push_str(&"─".repeat(fill_count));
            top.push('╮');
            lines.push(Line::from(Span::styled(
                top,
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )));
        }
    }

    for raw_line in raw_lines {
        let line_w = UnicodeWidthStr::width(raw_line);
        let mut spans = Vec::new();
        let mut current_text = String::new();
        let mut current_is_box = false;

        for ch in raw_line.chars() {
            let is_box = is_diagram_glyph(ch, opts.ascii);
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
            let pad = box_width.saturating_sub(line_w);
            let pad_str = " ".repeat(pad);
            let left_border = if opts.ascii { "| " } else { "│ " };
            let right_border = if opts.ascii { " |" } else { " │" };
            let border_color = if opts.ascii { Color::Cyan } else { Color::Blue };
            let side_style = Style::default().fg(border_color);

            let mut boxed_spans = vec![Span::styled(left_border, side_style)];
            boxed_spans.extend(spans);
            if !pad_str.is_empty() {
                boxed_spans.push(Span::raw(pad_str));
            }
            boxed_spans.push(Span::styled(right_border, side_style));
            lines.push(Line::from(boxed_spans));
        } else {
            lines.push(Line::from(spans));
        }
    }

    if opts.show_box {
        if opts.ascii {
            let mut bot = String::from("+");
            bot.push_str(&"-".repeat(box_width + 2));
            bot.push('+');
            lines.push(Line::from(Span::styled(bot, border_style)));
        } else {
            let mut bot = String::from("╰");
            bot.push_str(&"─".repeat(box_width + 2));
            bot.push('╯');
            lines.push(Line::from(Span::styled(
                bot,
                Style::default().fg(Color::Blue),
            )));
        }
    }

    Text::from(lines)
}

/// Check if a character is a diagram border, connector, or edge glyph.
fn is_diagram_glyph(ch: char, ascii: bool) -> bool {
    if ascii {
        return matches!(
            ch,
            '+' | '-' | '|' | '>' | '<' | '^' | 'v' | '\\' | '/' | '*'
        );
    }
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
            | '╒'
            | '╕'
            | '╘'
            | '╛'
            | '╞'
            | '╡'
            | '╤'
            | '╧'
            | '╪'
            | '╔'
            | '╗'
            | '╚'
            | '╝'
            | '╠'
            | '╣'
            | '╦'
            | '╩'
            | '╬'
            | '▲'
            | '▼'
            | '◄'
            | '►'
            | '▸'
            | '▾'
            | '▴'
            | '◀'
            | '▶'
            | '△'
            | '▽'
            | '▷'
            | '◁'
            | '◂'
            | '◆'
            | '◇'
            | '○'
            | '●'
            | '╱'
            | '╲'
            | '┄'
            | '┆'
            | '┈'
            | '┊'
            | '╌'
            | '╎'
            | '╍'
            | '╏'
            | '╴'
            | '╵'
            | '╶'
            | '╷'
            | '█'
            | '░'
            | '▒'
            | '▓'
    )
}

/// Render a diagram syntax error gracefully with a warning badge and the raw source.
fn render_mermaid_error(src: &str, error_msg: &str, opts: &MermaidOptions) -> Text<'static> {
    let mut lines = Vec::new();
    let err_border = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);

    let err_str = format!("Error: {error_msg}");
    let raw_lines: Vec<&str> = src.lines().collect();
    let content_w = err_str.len().max(
        raw_lines
            .iter()
            .map(|l| UnicodeWidthStr::width(*l))
            .max()
            .unwrap_or(0),
    );
    let box_width = content_w.max(34);

    if opts.ascii {
        let mut top = String::from("+-[ Mermaid Syntax Warning ]");
        let fill = (box_width + 4).saturating_sub(top.len() + 1);
        top.push_str(&"-".repeat(fill));
        top.push('+');
        lines.push(Line::from(Span::styled(top, err_border)));

        let pad_err = box_width.saturating_sub(err_str.len());
        lines.push(Line::from(vec![
            Span::styled("| ", err_border),
            Span::styled(err_str, Style::default().fg(Color::LightRed)),
            Span::raw(" ".repeat(pad_err)),
            Span::styled(" |", err_border),
        ]));

        let mut mid = String::from("+-[ Source ]");
        let fill_mid = (box_width + 4).saturating_sub(mid.len() + 1);
        mid.push_str(&"-".repeat(fill_mid));
        mid.push('+');
        lines.push(Line::from(Span::styled(mid, err_border)));

        for src_line in raw_lines {
            let line_w = UnicodeWidthStr::width(src_line);
            let pad = box_width.saturating_sub(line_w);
            lines.push(Line::from(vec![
                Span::styled("| ", err_border),
                Span::styled(src_line.to_string(), Style::default().fg(Color::DarkGray)),
                Span::raw(" ".repeat(pad)),
                Span::styled(" |", err_border),
            ]));
        }

        let mut bot = String::from("+");
        bot.push_str(&"-".repeat(box_width + 2));
        bot.push('+');
        lines.push(Line::from(Span::styled(bot, err_border)));
    } else {
        let mut top = String::from("╭─ [Mermaid Syntax Warning] ");
        let fill = (box_width + 4).saturating_sub(top.len() + 1);
        top.push_str(&"─".repeat(fill));
        top.push('╮');
        lines.push(Line::from(Span::styled(top, err_border)));

        let pad_err = box_width.saturating_sub(err_str.len());
        lines.push(Line::from(vec![
            Span::styled("│ ", err_border),
            Span::styled(err_str, Style::default().fg(Color::LightRed)),
            Span::raw(" ".repeat(pad_err)),
            Span::styled(" │", err_border),
        ]));

        let mut mid = String::from("├─ [Source] ");
        let fill_mid = (box_width + 4).saturating_sub(mid.len() + 1);
        mid.push_str(&"─".repeat(fill_mid));
        mid.push('┤');
        lines.push(Line::from(Span::styled(mid, err_border)));

        for src_line in raw_lines {
            let line_w = UnicodeWidthStr::width(src_line);
            let pad = box_width.saturating_sub(line_w);
            lines.push(Line::from(vec![
                Span::styled("│ ", err_border),
                Span::styled(src_line.to_string(), Style::default().fg(Color::DarkGray)),
                Span::raw(" ".repeat(pad)),
                Span::styled(" │", err_border),
            ]));
        }

        let mut bot = String::from("╰");
        bot.push_str(&"─".repeat(box_width + 2));
        bot.push('╯');
        lines.push(Line::from(Span::styled(bot, err_border)));
    }

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
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
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
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
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
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
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

    #[test]
    fn test_render_ascii_with_box_is_pure_ascii() {
        let src = "graph LR\n    A[Step 1] --> B[Step 2]";
        let opts = MermaidOptions {
            ascii: true,
            show_box: true,
            title: Some("ASCII Box Test".to_string()),
            ..Default::default()
        };
        let text = render_mermaid(src, &opts);
        let joined = text
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.is_ascii(),
            "Rendered ASCII diagram with box must be pure ASCII"
        );
        assert!(joined.contains("+-[ ASCII Box Test ]"));
        assert!(joined.contains("Step 1"));
        assert!(joined.contains("Step 2"));
    }

    #[test]
    fn test_render_box_symmetric_width() {
        let src =
            "graph LR\n    Alpha[Start Here] --> Beta[A Somewhat Longer Step] --> Gamma[Done]";
        let opts = MermaidOptions {
            show_box: true,
            title: Some("Symmetry Test".to_string()),
            ..Default::default()
        };
        let text = render_mermaid(src, &opts);
        let widths: Vec<usize> = text
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum()
            })
            .collect();
        assert!(!widths.is_empty());
        let expected_w = widths[0];
        for (idx, w) in widths.iter().enumerate() {
            assert_eq!(
                *w, expected_w,
                "Line {idx} width ({w}) does not match header width ({expected_w})"
            );
        }
    }

    #[test]
    fn test_render_class_diagram() {
        let src = "classDiagram\n    Animal <|-- Duck";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
        let joined = text
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Animal"));
        assert!(joined.contains("Duck"));
    }

    #[test]
    fn test_render_sequence_diagram() {
        let src = "sequenceDiagram\n    Alice->>Bob: Ping\n    Bob-->>Alice: Pong";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
        let joined = text
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Alice"));
        assert!(joined.contains("Bob"));
        assert!(joined.contains("Ping"));
        assert!(joined.contains("Pong"));
    }

    #[test]
    fn test_render_mermaid_to_image() {
        let src =
            "flowchart TD\n    A[Start] --> B{Choice}\n    B -->|Yes| C[OK]\n    B -->|No| D[Stop]";
        let img = render_mermaid_to_image(src, 1.0);
        assert!(
            img.is_some(),
            "Should successfully render flowchart to image"
        );
        let img = img.unwrap();
        assert!(img.width() > 0, "Image width must be positive");
        assert!(img.height() > 0, "Image height must be positive");

        // Test zoom / scaling
        let img_scaled = render_mermaid_to_image(src, 2.0).unwrap();
        assert!(
            img_scaled.width() > img.width(),
            "Scaled image width should be larger: {} vs {}",
            img_scaled.width(),
            img.width()
        );

        // Test LRU cache hit (should return identical dimensions instantly)
        let cached = render_mermaid_to_image(src, 1.0).unwrap();
        assert_eq!(cached.width(), img.width());
        assert_eq!(cached.height(), img.height());
    }

    #[test]
    fn test_render_mermaid_to_image_empty() {
        assert!(render_mermaid_to_image("", 1.0).is_none());
        assert!(render_mermaid_to_image("   \n  ", 1.0).is_none());
    }
}
