use indexmap::IndexMap;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use rustc_hash::FxBuildHasher;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use unicode_width::UnicodeWidthStr;

use super::mermaid::{
    MermaidOptions, ensure_tmux_passthrough, is_kitty_supported, is_kitty_terminal_supported,
    render_mermaid, render_mermaid_to_unicode_placeholders_with_options,
};
use super::text::text_to_ansi;
use crate::config::{DiagramBackground, DiagramTheme};

/// Options for Markdown rendering.
#[derive(Debug, Clone)]
pub struct MarkdownOptions {
    /// Maximum width / column budget for word wrapping and diagram layout.
    pub max_width: Option<usize>,
    /// Base directory or markdown file path for resolving relative image links.
    pub base_path: Option<PathBuf>,
    /// Whether to render embedded ```mermaid ... ``` code blocks as diagrams.
    pub render_mermaid: bool,
    /// Whether to force ASCII-only characters in rendered Mermaid diagrams.
    pub mermaid_ascii: bool,
    /// Whether to show line numbers in standard code blocks.
    pub show_line_numbers: bool,
    /// Whether to render embedded Mermaid diagrams as Kitty Graphics Protocol images.
    pub mermaid_image: bool,
    /// Whether to render embedded Mermaid diagrams inline using Kitty Unicode Placeholders.
    pub inline_diagrams: bool,
    /// Whether to render embedded local images inline using Kitty Unicode Placeholders.
    pub inline_images: bool,
    /// Diagram theme: Auto, Dark, or Light.
    pub diagram_theme: DiagramTheme,
    /// Diagram background mode: Transparent or Solid.
    pub diagram_background: DiagramBackground,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            max_width: None,
            base_path: None,
            render_mermaid: true,
            mermaid_ascii: false,
            show_line_numbers: false,
            mermaid_image: false,
            inline_diagrams: false,
            inline_images: true,
            diagram_theme: DiagramTheme::default(),
            diagram_background: DiagramBackground::default(),
        }
    }
}

/// Render a Markdown source string to Ratatui `Text<'static>`.
pub fn render_markdown(src: &str, opts: &MarkdownOptions) -> Text<'static> {
    render_markdown_with_diagram_offsets(src, opts).0
}

/// Render a Markdown source string to an ANSI colored string for terminal stdout.
pub fn render_markdown_ansi(src: &str, opts: &MarkdownOptions) -> String {
    let mut parser_opts = Options::empty();
    parser_opts.insert(Options::ENABLE_TABLES);
    parser_opts.insert(Options::ENABLE_TASKLISTS);
    parser_opts.insert(Options::ENABLE_STRIKETHROUGH);
    parser_opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    ensure_tmux_passthrough();

    let parser = Parser::new_ext(src, parser_opts);
    let mut renderer = MarkdownRenderer::new(opts);
    renderer.render(parser);
    let (text, _, transmissions) = renderer.finish_with_transmissions();
    let mut out = String::new();
    let mut seen_ids = std::collections::HashSet::new();
    for (id, t) in transmissions {
        if seen_ids.insert(id) {
            out.push_str(&t);
        }
    }
    out.push_str(&text_to_ansi(&text));
    out
}

/// Read a Markdown file and render it as Ratatui `Text<'static>`.
pub fn render_markdown_file(path: &Path, opts: &MarkdownOptions) -> anyhow::Result<Text<'static>> {
    let content = fs::read_to_string(path)?;
    let mut file_opts = opts.clone();
    if file_opts.base_path.is_none() {
        file_opts.base_path = Some(path.to_path_buf());
    }
    Ok(render_markdown(&content, &file_opts))
}

/// Read a Markdown file and render it as an ANSI colored string for terminal stdout.
pub fn render_markdown_file_ansi(path: &Path, opts: &MarkdownOptions) -> anyhow::Result<String> {
    let content = fs::read_to_string(path)?;
    let mut file_opts = opts.clone();
    if file_opts.base_path.is_none() {
        file_opts.base_path = Some(path.to_path_buf());
    }
    Ok(render_markdown_ansi(&content, &file_opts))
}

/// Extract all ` ```mermaid ` blocks from raw Markdown source.
///
/// Returns a `Vec<(start_line, source)>` where `start_line` is the 0-based index of the opening
/// fence line in the **source** (not the rendered output — the previewer must map this via the
/// rendered line count).  The source string contains only the diagram body (no fence lines).
pub fn extract_mermaid_blocks(src: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut block_start_line = 0usize;
    let mut buf = String::new();

    for (line_idx, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if !in_block && (trimmed.starts_with("```mermaid") || trimmed.starts_with("~~~mermaid")) {
            in_block = true;
            block_start_line = line_idx;
            buf.clear();
        } else if in_block && (trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            in_block = false;
            blocks.push((block_start_line, buf.trim().to_owned()));
            buf.clear();
        } else if in_block {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    blocks
}

/// Render Markdown and also return the **rendered** line offsets of each embedded Mermaid block.
///
/// The offsets are the indices into the resulting `Text::lines` slice so that callers can jump the
/// preview scroll directly to the right line.
pub fn render_markdown_with_diagram_offsets(
    src: &str,
    opts: &MarkdownOptions,
) -> (Text<'static>, Vec<usize>) {
    let mut parser_opts = Options::empty();
    parser_opts.insert(Options::ENABLE_TABLES);
    parser_opts.insert(Options::ENABLE_TASKLISTS);
    parser_opts.insert(Options::ENABLE_STRIKETHROUGH);
    parser_opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let parser = Parser::new_ext(src, parser_opts);
    let mut renderer = MarkdownRenderer::new(opts);
    renderer.render(parser);
    renderer.finish()
}

/// Read a Markdown file, render it, and also return diagram line offsets.
pub fn render_markdown_file_with_diagram_offsets(
    path: &Path,
    opts: &MarkdownOptions,
) -> anyhow::Result<(Text<'static>, Vec<usize>)> {
    let content = fs::read_to_string(path)?;
    let mut file_opts = opts.clone();
    if file_opts.base_path.is_none() {
        file_opts.base_path = Some(path.to_path_buf());
    }
    Ok(render_markdown_with_diagram_offsets(&content, &file_opts))
}

/// Check whether inline image rendering is enabled.
/// Precedence:
/// 1. Explicit `MM_INLINE_IMAGES` environment variable override ("0"/"1", etc.).
/// 2. `opts_inline_images && is_kitty_terminal_supported()`.
pub fn is_inline_images_enabled(opts_inline_images: bool) -> bool {
    if let Ok(val) = std::env::var("MM_INLINE_IMAGES") {
        let v = val.trim();
        if v == "0"
            || v.eq_ignore_ascii_case("false")
            || v.eq_ignore_ascii_case("off")
            || v.eq_ignore_ascii_case("no")
        {
            return false;
        }
        if v == "1"
            || v.eq_ignore_ascii_case("true")
            || v.eq_ignore_ascii_case("on")
            || v.eq_ignore_ascii_case("yes")
        {
            return true;
        }
    }
    opts_inline_images && is_kitty_terminal_supported()
}

/// Key for LRU caching of rendered image placeholders.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageCacheKey {
    pub path_or_data: String,
    pub mtime_nanos: Option<u128>,
    pub file_size: u64,
    pub max_cols: u32,
    pub max_rows: u32,
}

pub type KittyPlaceholderImage = crate::utils::mermaid::KittyPlaceholderDiagram;

static IMAGE_PLACEHOLDER_CACHE: LazyLock<
    Mutex<IndexMap<ImageCacheKey, KittyPlaceholderImage, FxBuildHasher>>,
> = LazyLock::new(|| Mutex::new(IndexMap::with_capacity_and_hasher(64, FxBuildHasher)));

#[cfg(test)]
pub fn clear_image_placeholder_cache() {
    if let Ok(mut cache) = IMAGE_PLACEHOLDER_CACHE.lock() {
        cache.clear();
    }
}

fn url_decode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    result.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

/// Resolves a local image path from a URL (e.g. relative path, absolute path, file:// URI, tilde path).
/// Returns `None` if the path is a remote HTTP/HTTPS URL, a data URI, or does not exist on disk.
pub fn resolve_local_image_path(url: &str, base_path: Option<&Path>) -> Option<PathBuf> {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:") {
        return None;
    }

    let raw = if let Some(stripped) = url.strip_prefix("file://") {
        if let Some(rest) = stripped.strip_prefix("localhost") {
            rest
        } else {
            stripped
        }
    } else if let Some(stripped) = url.strip_prefix("file:") {
        stripped
    } else {
        url
    };

    // Strip fragment (#...) and query (?...)
    let no_hash = raw.split('#').next().unwrap_or(raw);
    let no_query = no_hash.split('?').next().unwrap_or(no_hash);
    let decoded = url_decode(no_query);

    let candidates = [
        // 1. Tilde expanded if applicable
        if let Some(rest) = decoded.strip_prefix("~/") {
            dirs::home_dir().map(|h| h.join(rest))
        } else {
            None
        },
        // 2. Decoded path
        Some(PathBuf::from(&decoded)),
        // 3. Raw path (if decoding was different)
        if decoded != no_query {
            Some(PathBuf::from(no_query))
        } else {
            None
        },
    ];

    for candidate in candidates.into_iter().flatten() {
        // If candidate is absolute, check directly
        if candidate.is_absolute() {
            if candidate.is_file() {
                return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
            }
            continue;
        }

        // For relative paths, prioritize resolving relative to base_path directory
        if let Some(base) = base_path {
            let base_dir = if base.is_dir() {
                base.to_path_buf()
            } else if let Some(parent) = base.parent() {
                if parent.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    parent.to_path_buf()
                }
            } else {
                base.to_path_buf()
            };

            let joined = base_dir.join(&candidate);
            if joined.is_file() {
                return Some(std::fs::canonicalize(&joined).unwrap_or(joined));
            }
        }

        // Fallback: check relative to CWD
        if candidate.is_file() {
            return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }

    None
}

fn decode_data_uri(url: &str) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("data:")?;
    let (_, data) = rest.split_once("base64,")?;
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()
}

/// Render a local image file or data URI to Kitty Unicode Placeholders (`\u{10EEEE}`).
pub fn render_image_to_unicode_placeholders(
    url: &str,
    base_path: Option<&Path>,
    max_width: Option<usize>,
) -> Option<KittyPlaceholderImage> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Compute terminal metrics and dimensions
    let (cell_w, cell_h) = match crossterm::terminal::window_size() {
        Ok(ws) if ws.columns > 0 && ws.rows > 0 && ws.width > 0 && ws.height > 0 => {
            (ws.width as f32 / ws.columns as f32, ws.height as f32 / ws.rows as f32)
        }
        _ => (10.0, 20.0),
    };
    let cell_w = if cell_w > 0.0 { cell_w } else { 10.0 };
    let cell_h = if cell_h > 0.0 { cell_h } else { 20.0 };

    let term_cols = match crossterm::terminal::window_size() {
        Ok(ws) if ws.columns > 0 => ws.columns as usize,
        _ => match crossterm::terminal::size() {
            Ok((cols, _)) if cols > 0 => cols as usize,
            _ => 80,
        },
    };

    let term_rows = match crossterm::terminal::window_size() {
        Ok(ws) if ws.rows > 0 => ws.rows as u32,
        _ => match crossterm::terminal::size() {
            Ok((_, rows)) if rows > 0 => rows as u32,
            _ => 40,
        },
    };

    let available_width = max_width.unwrap_or(term_cols);
    let budget_width = available_width.saturating_sub(4).max(10) as u32;
    let max_diacritics = 255u32;
    let max_cols = budget_width.min(max_diacritics);
    let max_rows = term_rows.saturating_sub(2).clamp(1, 80).min(max_diacritics);

    // Determine cache key
    let (cache_key, is_data, resolved_path) = if trimmed.starts_with("data:") {
        use rustc_hash::FxHasher;
        use std::hash::Hasher;
        let mut hasher = FxHasher::default();
        std::hash::Hash::hash(trimmed, &mut hasher);
        let key = ImageCacheKey {
            path_or_data: format!("data:{:x}", hasher.finish()),
            mtime_nanos: None,
            file_size: trimmed.len() as u64,
            max_cols,
            max_rows,
        };
        (key, true, None)
    } else {
        let resolved = resolve_local_image_path(trimmed, base_path)?;
        let meta = fs::metadata(&resolved).ok()?;
        let mtime_nanos = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());
        let key = ImageCacheKey {
            path_or_data: resolved.to_string_lossy().to_string(),
            mtime_nanos,
            file_size: meta.len(),
            max_cols,
            max_rows,
        };
        (key, false, Some(resolved))
    };

    // 2. Check LRU cache first
    if let Ok(mut cache) = IMAGE_PLACEHOLDER_CACHE.lock() {
        if let Some((_, item)) = cache.shift_remove_entry(&cache_key) {
            let cloned = item.clone();
            cache.insert(cache_key, item);
            return Some(cloned);
        }
    }

    // 3. Load image
    let img = if is_data {
        let bytes = decode_data_uri(trimmed)?;
        if trimmed.starts_with("data:image/svg") {
            let svg_str = std::str::from_utf8(&bytes).ok()?;
            crate::utils::mermaid::render_svg_to_image(svg_str, 1.5)?
        } else {
            image::load_from_memory(&bytes).ok()?
        }
    } else {
        let resolved = resolved_path.as_deref().unwrap_or_else(|| Path::new(&cache_key.path_or_data));
        let is_svg = resolved
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("svg"))
            .unwrap_or(false);

        if is_svg {
            let svg_content = fs::read_to_string(resolved).ok()?;
            crate::utils::mermaid::render_svg_to_image(&svg_content, 1.5)?
        } else {
            image::open(resolved).ok()?
        }
    };

    let img_w = img.width() as f32;
    let img_h = img.height() as f32;
    if img_w <= 0.0 || img_h <= 0.0 {
        return None;
    }

    let natural_cols = (img_w / cell_w).round().max(1.0) as u32;
    let natural_rows = (img_h / cell_h).round().max(1.0) as u32;

    let scale_w = (max_cols as f32) / (natural_cols as f32);
    let scale_h = (max_rows as f32) / (natural_rows as f32);
    let scale = scale_w.min(scale_h).min(1.0);

    let cols = ((natural_cols as f32 * scale).round() as u32).clamp(1, max_cols);
    let rows = ((natural_rows as f32 * scale).round() as u32).clamp(1, max_rows);

    // 4. Bound pixel resolution before PNG encoding for fast transmission
    let target_pixel_w = (cols * 24).clamp(64, 1920);
    let target_pixel_h = (rows * 48).clamp(64, 1920);
    let img = if img.width() > target_pixel_w || img.height() > target_pixel_h {
        img.resize(target_pixel_w, target_pixel_h, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    // 5. Encode to PNG
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    ).ok()?;

    // 6. Generate distinct image ID
    let image_id = crate::utils::mermaid::next_diagram_image_id();

    // 7. Build transmission sequence
    let transmission = crate::utils::mermaid::encode_kitty_unicode_transmission(
        &png_bytes,
        image_id,
        cols,
        rows,
    );

    // 8. Generate placeholder lines
    let lines = crate::utils::mermaid::create_unicode_placeholder_lines(
        image_id,
        cols,
        rows,
        0,
    );

    let result = KittyPlaceholderImage {
        image_id,
        cols,
        rows,
        transmission,
        lines,
    };

    // 9. Insert into LRU cache
    if let Ok(mut cache) = IMAGE_PLACEHOLDER_CACHE.lock() {
        if cache.len() >= 64 {
            cache.shift_remove_index(0);
        }
        cache.insert(cache_key, result.clone());
    }

    Some(result)
}

struct ListState {
    is_ordered: bool,
    counter: u64,
}

struct TableState {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_head: bool,
}

enum CodeBlockState {
    Mermaid { buffer: String },
    Code { lang: String, buffer: String },
}

fn is_line_blank(line: &Line<'_>) -> bool {
    line.spans.is_empty() || line.spans.iter().all(|s| s.content.trim().is_empty())
}

fn wrap_spans_to_lines(
    spans: Vec<Span<'static>>,
    max_width: usize,
    continuation_indent: usize,
) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 {
        return vec![spans];
    }

    let total_w: usize = spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    if total_w <= max_width {
        return vec![spans];
    }

    let mut result: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut current_line_width: usize = 0;

    for span in spans {
        let style = span.style;
        let content = span.content;

        let mut char_indices = content.char_indices().peekable();
        while let Some(&(start, ch)) = char_indices.peek() {
            let is_space = ch.is_whitespace();
            let mut end = start + ch.len_utf8();
            char_indices.next();

            while let Some(&(next_start, next_ch)) = char_indices.peek() {
                if next_ch.is_whitespace() == is_space {
                    end = next_start + next_ch.len_utf8();
                    char_indices.next();
                } else {
                    break;
                }
            }

            let segment = &content[start..end];
            let seg_w = UnicodeWidthStr::width(segment);

            if is_space {
                if current_line_width == 0 {
                    continue;
                }
                if current_line_width + seg_w > max_width {
                    result.push(std::mem::take(&mut current_line));
                    current_line_width = 0;
                    continue;
                }
                current_line.push(Span::styled(segment.to_string(), style));
                current_line_width += seg_w;
            } else {
                if current_line_width > 0 && current_line_width + seg_w > max_width {
                    result.push(std::mem::take(&mut current_line));
                    current_line_width = 0;

                    if continuation_indent > 0 && continuation_indent < max_width {
                        let indent_str = " ".repeat(continuation_indent);
                        current_line.push(Span::raw(indent_str));
                        current_line_width = continuation_indent;
                    }
                }

                current_line.push(Span::styled(segment.to_string(), style));
                current_line_width += seg_w;
            }
        }
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    if result.is_empty() {
        vec![Vec::new()]
    } else {
        result
    }
}

struct MarkdownRenderer<'a> {
    opts: &'a MarkdownOptions,
    lines: Vec<Line<'static>>,
    current_line: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    heading_level: Option<HeadingLevel>,
    list_stack: Vec<ListState>,
    pending_item_prefix: Option<(usize, Span<'static>)>,
    list_continuation_indent: usize,
    blockquote_depth: usize,
    code_block: Option<CodeBlockState>,
    table: Option<TableState>,
    current_link_url: Option<String>,
    current_link_text: String,
    current_image_url: Option<String>,
    current_image_alt: String,
    diagram_offsets: Vec<usize>,
    transmissions: Vec<(u32, String)>,
}

impl<'a> MarkdownRenderer<'a> {
    fn new(opts: &'a MarkdownOptions) -> Self {
        Self {
            opts,
            lines: Vec::new(),
            current_line: Vec::new(),
            style_stack: vec![Style::default()],
            heading_level: None,
            list_stack: Vec::new(),
            pending_item_prefix: None,
            list_continuation_indent: 0,
            blockquote_depth: 0,
            code_block: None,
            table: None,
            current_link_url: None,
            current_link_text: String::new(),
            current_image_url: None,
            current_image_alt: String::new(),
            diagram_offsets: Vec::new(),
            transmissions: Vec::new(),
        }
    }

    fn current_style(&self) -> Style {
        *self.style_stack.last().unwrap_or(&Style::default())
    }

    fn push_style(&mut self, patch: Style) {
        let new_style = self.current_style().patch(patch);
        self.style_stack.push(new_style);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn ensure_pending_prefix(&mut self) {
        if let Some((depth, bullet)) = self.pending_item_prefix.take() {
            let mut prefix = Vec::new();
            if depth > 0 {
                prefix.push(Span::raw("  ".repeat(depth)));
            }
            prefix.push(bullet);
            self.current_line.splice(0..0, prefix);
        }
    }

    fn flush_line(&mut self) {
        if self.current_line.is_empty() {
            return;
        }
        self.ensure_pending_prefix();
        let spans = std::mem::take(&mut self.current_line);
        let continuation = self.list_continuation_indent;

            if let Some(mw) = self.opts.max_width {
                let quote_overhead = self.blockquote_depth * 2;
                let effective_width = mw.saturating_sub(quote_overhead);
                if effective_width >= 10 {
                    let wrapped_groups = wrap_spans_to_lines(spans, effective_width, continuation);
                    for group in wrapped_groups {
                        if self.blockquote_depth > 0 {
                            let quote_prefix = Span::styled(
                                "▌ ".repeat(self.blockquote_depth),
                                Style::default().fg(Color::Cyan),
                            );
                            let mut full = vec![quote_prefix];
                            full.extend(group);
                            self.lines.push(Line::from(full));
                        } else {
                            self.lines.push(Line::from(group));
                        }
                    }
                    return;
                }
            }

            if self.blockquote_depth > 0 {
                let quote_prefix = Span::styled(
                    "▌ ".repeat(self.blockquote_depth),
                    Style::default().fg(Color::Cyan),
                );
                let mut full_spans = vec![quote_prefix];
                full_spans.extend(spans);
                self.lines.push(Line::from(full_spans));
            } else {
                self.lines.push(Line::from(spans));
            }
    }

    fn ensure_blank_line(&mut self) {
        self.flush_line();
        if let Some(last) = self.lines.last() {
            if !is_line_blank(last) {
                if self.blockquote_depth > 0 {
                    let quote_prefix = Span::styled(
                        "▌ ".repeat(self.blockquote_depth),
                        Style::default().fg(Color::Cyan),
                    );
                    self.lines.push(Line::from(vec![quote_prefix]));
                } else {
                    self.lines.push(Line::default());
                }
            }
        }
    }

    fn render(&mut self, parser: Parser) {
        for event in parser {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: Event) {
        // If we are accumulating code block contents:
        if let Some(ref mut cb) = self.code_block {
            match event {
                Event::Text(text) => match cb {
                    CodeBlockState::Mermaid { buffer } => buffer.push_str(&text),
                    CodeBlockState::Code { buffer, .. } => buffer.push_str(&text),
                },
                Event::End(TagEnd::CodeBlock) => {
                    let cb_taken = self.code_block.take().unwrap();
                    self.finish_code_block(cb_taken);
                }
                _ => {}
            }
            return;
        }

        // If we are accumulating table contents:
        if let Some(ref mut tbl) = self.table {
            match event {
                Event::Start(Tag::TableHead) => {
                    tbl.in_head = true;
                    tbl.current_row.clear();
                }
                Event::End(TagEnd::TableHead) => {
                    tbl.in_head = false;
                    let row = std::mem::take(&mut tbl.current_row);
                    tbl.rows.push(row);
                }
                Event::Start(Tag::TableRow) => {
                    tbl.current_row.clear();
                }
                Event::End(TagEnd::TableRow) => {
                    let row = std::mem::take(&mut tbl.current_row);
                    tbl.rows.push(row);
                }
                Event::Start(Tag::TableCell) => {
                    tbl.current_cell.clear();
                }
                Event::End(TagEnd::TableCell) => {
                    let cell = std::mem::take(&mut tbl.current_cell);
                    tbl.current_row.push(cell);
                }
                Event::Text(text) => {
                    tbl.current_cell.push_str(&text);
                }
                Event::Code(code) => {
                    tbl.current_cell.push_str(&code);
                }
                Event::End(TagEnd::Table) => {
                    let tbl_taken = self.table.take().unwrap();
                    self.finish_table(tbl_taken);
                }
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(tag) => self.handle_start_tag(tag),
            Event::End(tag_end) => self.handle_end_tag(tag_end),
            Event::Text(text) => {
                if self.current_image_url.is_some() {
                    self.current_image_alt.push_str(&text);
                    return;
                }
                self.ensure_pending_prefix();
                if self.current_link_url.is_some() {
                    self.current_link_text.push_str(&text);
                }
                let style = self.current_style();
                self.current_line
                    .push(Span::styled(text.into_string(), style));
            }
            Event::Code(code) => {
                if self.current_image_url.is_some() {
                    self.current_image_alt.push_str(&code);
                    return;
                }
                self.ensure_pending_prefix();
                let code_style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(35, 38, 46));
                self.current_line
                    .push(Span::styled(format!(" `{code}` "), code_style));
            }
            Event::TaskListMarker(checked) => {
                let depth = if let Some((d, _)) = self.pending_item_prefix.take() {
                    d
                } else {
                    self.list_stack.len().saturating_sub(1)
                };
                let indent = "  ".repeat(depth);
                if !indent.is_empty() {
                    self.current_line.push(Span::raw(indent));
                }
                self.list_continuation_indent = depth * 2 + 4;
                if checked {
                    self.current_line.push(Span::styled(
                        "[✓] ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    self.current_line
                        .push(Span::styled("[ ] ", Style::default().fg(Color::DarkGray)));
                }
            }
            Event::Rule => {
                self.ensure_blank_line();
                let rule_width = self.opts.max_width.unwrap_or(60).min(72);
                let rule_str = "─".repeat(rule_width);
                self.lines.push(Line::from(Span::styled(
                    rule_str,
                    Style::default().fg(Color::DarkGray),
                )));
                self.lines.push(Line::default());
            }
            Event::SoftBreak => {
                self.current_line.push(Span::raw(" "));
            }
            Event::HardBreak => {
                self.flush_line();
            }
            _ => {}
        }
    }

    fn handle_start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if self.pending_item_prefix.is_none() {
                    self.ensure_blank_line();
                }
            }
            Tag::Heading { level, .. } => {
                self.ensure_blank_line();
                self.heading_level = Some(level);
                let (color, prefix) = match level {
                    HeadingLevel::H1 => (Color::Cyan, "# "),
                    HeadingLevel::H2 => (Color::Yellow, "## "),
                    HeadingLevel::H3 => (Color::Green, "### "),
                    HeadingLevel::H4 => (Color::Magenta, "#### "),
                    HeadingLevel::H5 => (Color::Blue, "##### "),
                    HeadingLevel::H6 => (Color::DarkGray, "###### "),
                };
                self.push_style(Style::default().fg(color).add_modifier(Modifier::BOLD));
                self.current_line.push(Span::styled(
                    prefix,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ));
            }
            Tag::BlockQuote(_) => {
                self.ensure_blank_line();
                self.blockquote_depth += 1;
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.ensure_blank_line();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.trim().to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                if (lang.eq_ignore_ascii_case("mermaid") || lang.eq_ignore_ascii_case("mmd"))
                    && self.opts.render_mermaid
                {
                    self.code_block = Some(CodeBlockState::Mermaid {
                        buffer: String::new(),
                    });
                } else {
                    self.code_block = Some(CodeBlockState::Code {
                        lang,
                        buffer: String::new(),
                    });
                }
            }
            Tag::List(opt_start) => {
                if self.list_stack.is_empty() {
                    self.ensure_blank_line();
                } else {
                    self.flush_line();
                }
                self.list_stack.push(ListState {
                    is_ordered: opt_start.is_some(),
                    counter: opt_start.unwrap_or(1),
                });
            }
            Tag::Item => {
                self.flush_line();
                let depth = self.list_stack.len().saturating_sub(1);
                if let Some(list_state) = self.list_stack.last_mut() {
                    if list_state.is_ordered {
                        let num = list_state.counter;
                        list_state.counter += 1;
                        let prefix_str = format!("{num}. ");
                        let w = UnicodeWidthStr::width(prefix_str.as_str());
                        self.list_continuation_indent = depth * 2 + w;
                        let bullet_span =
                            Span::styled(prefix_str, Style::default().fg(Color::Cyan));
                        self.pending_item_prefix = Some((depth, bullet_span));
                    } else {
                        let bullet = match depth % 3 {
                            0 => "• ",
                            1 => "◦ ",
                            _ => "▪ ",
                        };
                        let bullet_color = match depth % 3 {
                            0 => Color::Cyan,
                            1 => Color::Yellow,
                            _ => Color::DarkGray,
                        };
                        self.list_continuation_indent = depth * 2 + 2;
                        let bullet_span = Span::styled(bullet, Style::default().fg(bullet_color));
                        self.pending_item_prefix = Some((depth, bullet_span));
                    }
                }
            }
            Tag::Table(alignments) => {
                self.ensure_blank_line();
                self.table = Some(TableState {
                    alignments,
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: String::new(),
                    in_head: false,
                });
            }
            Tag::Emphasis => {
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::Strong => {
                self.push_style(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { dest_url, .. } => {
                self.current_link_url = Some(dest_url.to_string());
                self.current_link_text.clear();
                self.push_style(
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            Tag::Image { dest_url, .. } => {
                self.current_image_url = Some(dest_url.to_string());
                self.current_image_alt.clear();
            }
            _ => {}
        }
    }

    fn handle_end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Paragraph => {
                self.flush_line();
            }
            TagEnd::Heading(_) => {
                self.flush_line();
                self.pop_style();
                self.heading_level = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_line();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.pop_style();
            }
            TagEnd::List(_) => {
                self.flush_line();
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.lines.push(Line::default());
                }
            }
            TagEnd::Item => {
                self.flush_line();
                self.list_continuation_indent = 0;
                self.pending_item_prefix = None;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.pop_style();
            }
            TagEnd::Link => {
                self.pop_style();
                if let Some(url) = self.current_link_url.take()
                    && !url.is_empty()
                    && url != self.current_link_text
                {
                    self.current_line.push(Span::styled(
                        format!(" ({url})"),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                self.current_link_text.clear();
            }
            TagEnd::Image => {
                let url = self.current_image_url.take().unwrap_or_default();
                let alt = std::mem::take(&mut self.current_image_alt);

                let use_inline = is_inline_images_enabled(self.opts.inline_images);
                if use_inline {
                    if let Some(img) = render_image_to_unicode_placeholders(
                        &url,
                        self.opts.base_path.as_deref(),
                        self.opts.max_width,
                    ) {
                        self.flush_line();
                        self.transmissions.push((img.image_id, img.transmission));
                        self.ensure_blank_line();

                        let opt_pending = self.pending_item_prefix.take();
                        let depth = self.list_stack.len().saturating_sub(1);
                        let base_indent = if opt_pending.is_some() || depth > 0 {
                            depth * 2
                        } else {
                            0
                        };

                        for (idx, line) in img.lines.into_iter().enumerate() {
                            let mut spans = Vec::new();
                            if self.blockquote_depth > 0 {
                                spans.push(Span::styled(
                                    "▌ ".repeat(self.blockquote_depth),
                                    Style::default().fg(Color::Cyan),
                                ));
                            }

                            if let Some((_, ref bullet)) = opt_pending {
                                if idx == 0 {
                                    if base_indent > 0 {
                                        spans.push(Span::raw(" ".repeat(base_indent)));
                                    }
                                    spans.push(bullet.clone());
                                } else {
                                    let bullet_w = UnicodeWidthStr::width(bullet.content.as_ref());
                                    spans.push(Span::raw(" ".repeat(base_indent + bullet_w)));
                                }
                            } else if base_indent > 0 {
                                spans.push(Span::raw(" ".repeat(base_indent)));
                            }

                            spans.extend(line.spans);
                            self.lines.push(Line::from(spans));
                        }
                        self.lines.push(Line::default());
                        return;
                    }
                }

                self.ensure_pending_prefix();
                let label = if alt.is_empty() { "image" } else { &alt };
                let img_span = Span::styled(
                    format!("🖼  [{label}]({url})"),
                    Style::default()
                        .fg(Color::LightMagenta)
                        .add_modifier(Modifier::ITALIC),
                );
                self.current_line.push(img_span);
            }
            _ => {}
        }
    }

    fn finish_code_block(&mut self, cb: CodeBlockState) {
        match cb {
            CodeBlockState::Mermaid { buffer } => {
                let use_inline = (self.opts.inline_diagrams || self.opts.mermaid_image)
                    && !self.opts.mermaid_ascii
                    && is_kitty_supported();
                if use_inline {
                    if let Some(diag) = render_mermaid_to_unicode_placeholders_with_options(
                        &buffer,
                        self.opts.max_width,
                        self.opts.diagram_theme,
                        self.opts.diagram_background,
                    ) {
                        self.transmissions.push((diag.image_id, diag.transmission));
                        self.ensure_blank_line();
                        self.diagram_offsets.push(self.lines.len());
                        for line in diag.lines {
                            self.lines.push(line);
                        }
                        self.lines.push(Line::default());
                        return;
                    }
                }
                let mermaid_opts = MermaidOptions {
                    max_width: self.opts.max_width,
                    ascii: self.opts.mermaid_ascii,
                    show_box: true,
                    title: Some("Mermaid Diagram (Press 's' to view image)".to_string()),
                    inline_diagrams: false,
                    theme: self.opts.diagram_theme,
                    background: self.opts.diagram_background,
                };
                let diagram_text = render_mermaid(&buffer, &mermaid_opts);
                self.diagram_offsets.push(self.lines.len());
                for line in diagram_text.lines {
                    self.lines.push(line);
                }
                self.lines.push(Line::default());
            }
            CodeBlockState::Code { lang, buffer } => {
                let border_style = Style::default().fg(Color::DarkGray);
                let lang_tag = if lang.is_empty() {
                    "code".to_string()
                } else {
                    lang
                };
                let raw_lines: Vec<&str> = buffer.lines().collect();
                let line_count = raw_lines.len();

                let title_w = UnicodeWidthStr::width(lang_tag.as_str());
                let content_w = raw_lines
                    .iter()
                    .map(|l| {
                        let base = UnicodeWidthStr::width(*l);
                        if self.opts.show_line_numbers && line_count > 1 {
                            base + 6
                        } else {
                            base
                        }
                    })
                    .max()
                    .unwrap_or(0);
                let inner_width = content_w.max(title_w + 4);
                let box_width = if let Some(mw) = self.opts.max_width {
                    inner_width.min(mw.saturating_sub(4).max(inner_width.min(20)))
                } else {
                    inner_width
                };

                let prefix = format!("╭─ [{lang_tag}] ");
                let p_len = UnicodeWidthStr::width(prefix.as_str());
                let fill_count = (box_width + 4).saturating_sub(p_len + 1);
                let mut top = prefix;
                top.push_str(&"─".repeat(fill_count));
                top.push('╮');
                self.lines.push(Line::from(Span::styled(top, border_style)));

                for (idx, line_str) in raw_lines.iter().enumerate() {
                    let mut spans = vec![Span::styled("│ ", border_style)];
                    let mut line_w = 0;
                    if self.opts.show_line_numbers && line_count > 1 {
                        let num_str = format!("{:>3} │ ", idx + 1);
                        line_w += UnicodeWidthStr::width(num_str.as_str());
                        spans.push(Span::styled(num_str, Style::default().fg(Color::DarkGray)));
                    }
                    line_w += UnicodeWidthStr::width(*line_str);
                    spans.push(Span::styled(
                        line_str.to_string(),
                        Style::default().fg(Color::LightYellow),
                    ));

                    let pad = box_width.saturating_sub(line_w);
                    if pad > 0 {
                        spans.push(Span::raw(" ".repeat(pad)));
                    }
                    spans.push(Span::styled(" │", border_style));
                    self.lines.push(Line::from(spans));
                }

                let mut bot = String::from("╰");
                bot.push_str(&"─".repeat(box_width + 2));
                bot.push('╯');
                self.lines.push(Line::from(Span::styled(bot, border_style)));
                self.lines.push(Line::default());
            }
        }
    }

    fn finish_table(&mut self, tbl: TableState) {
        if tbl.rows.is_empty() {
            return;
        }

        let num_cols = tbl
            .alignments
            .len()
            .max(tbl.rows.iter().map(|r| r.len()).max().unwrap_or(0));
        if num_cols == 0 {
            return;
        }

        let mut col_widths = vec![3usize; num_cols];
        for row in &tbl.rows {
            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx < num_cols {
                    col_widths[col_idx] = col_widths[col_idx].max(cell.width());
                }
            }
        }

        // Apply max_width budgeting if set
        if let Some(max_w) = self.opts.max_width {
            let border_overhead = 3 * num_cols + 1;
            if max_w > border_overhead {
                let max_content_budget = max_w - border_overhead;
                while col_widths.iter().sum::<usize>() > max_content_budget {
                    if let Some((max_idx, &max_val)) =
                        col_widths.iter().enumerate().max_by_key(|entry| *entry.1)
                    {
                        if max_val <= 3 {
                            break;
                        }
                        col_widths[max_idx] -= 1;
                    } else {
                        break;
                    }
                }
            }
        }

        let border_style = Style::default().fg(Color::DarkGray);
        let header_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let cell_style = Style::default().fg(Color::White);

        // Build top border: ┌──────┬──────┐
        let mut top_border = String::from("┌");
        for (i, w) in col_widths.iter().enumerate() {
            top_border.push_str(&"─".repeat(*w + 2));
            if i + 1 < num_cols {
                top_border.push('┬');
            } else {
                top_border.push('┐');
            }
        }
        self.lines
            .push(Line::from(Span::styled(top_border, border_style)));

        for (row_idx, row) in tbl.rows.iter().enumerate() {
            let is_header = row_idx == 0;
            let mut row_spans = vec![Span::styled("│", border_style)];

            for (col_idx, &width) in col_widths.iter().enumerate().take(num_cols) {
                let cell_content = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                let align = tbl
                    .alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(Alignment::Left);
                let padded = pad_cell(cell_content, width, align);

                let style = if is_header { header_style } else { cell_style };
                row_spans.push(Span::styled(format!(" {padded} "), style));
                row_spans.push(Span::styled("│", border_style));
            }
            self.lines.push(Line::from(row_spans));

            if is_header && tbl.rows.len() > 1 {
                // Separator: ├──────┼──────┤
                let mut sep = String::from("├");
                for (i, w) in col_widths.iter().enumerate() {
                    sep.push_str(&"─".repeat(*w + 2));
                    if i + 1 < num_cols {
                        sep.push('┼');
                    } else {
                        sep.push('┤');
                    }
                }
                self.lines.push(Line::from(Span::styled(sep, border_style)));
            }
        }

        // Bottom border: └──────┴──────┘
        let mut bot_border = String::from("└");
        for (i, w) in col_widths.iter().enumerate() {
            bot_border.push_str(&"─".repeat(*w + 2));
            if i + 1 < num_cols {
                bot_border.push('┴');
            } else {
                bot_border.push('┘');
            }
        }
        self.lines
            .push(Line::from(Span::styled(bot_border, border_style)));
        self.lines.push(Line::default());
    }

    fn finish(mut self) -> (Text<'static>, Vec<usize>) {
        self.flush_line();
        for (image_id, seq) in &self.transmissions {
            crate::utils::mermaid::transmit_kitty_image_idempotent(*image_id, seq);
        }
        (Text::from(self.lines), self.diagram_offsets)
    }

    fn finish_with_transmissions(mut self) -> (Text<'static>, Vec<usize>, Vec<(u32, String)>) {
        self.flush_line();
        (Text::from(self.lines), self.diagram_offsets, self.transmissions)
    }
}

fn truncate_to_width(s: &str, target_width: usize) -> String {
    if target_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(s) <= target_width {
        return s.to_string();
    }
    if target_width == 1 {
        return "…".to_string();
    }
    let limit = target_width - 1;
    let mut cur_w = 0;
    let mut out = String::new();
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if cur_w + cw > limit {
            break;
        }
        out.push(ch);
        cur_w += cw;
    }
    out.push('…');
    out
}

fn pad_cell(content: &str, width: usize, align: Alignment) -> String {
    let content_len = content.width();
    if content_len > width {
        return truncate_to_width(content, width);
    }
    let diff = width - content_len;
    match align {
        Alignment::Left | Alignment::None => format!("{}{}", content, " ".repeat(diff)),
        Alignment::Right => format!("{}{}", " ".repeat(diff), content),
        Alignment::Center => {
            let left = diff / 2;
            let right = diff - left;
            format!("{}{}{}", " ".repeat(left), content, " ".repeat(right))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_basic() {
        let md = "# Title\n\nThis is **bold** and *italic* text.\n\n- Item 1\n- Item 2";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
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
        assert!(joined.contains("Title"));
        assert!(joined.contains("bold"));
        assert!(joined.contains("italic"));
        assert!(joined.contains("Item 1"));
    }

    #[test]
    fn test_render_markdown_embedded_mermaid() {
        let md = "# Architecture\n\n```mermaid\ngraph LR\n    A[Client] --> B[Server]\n```\n\nNext steps.";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
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
        assert!(joined.contains("Architecture"));
        assert!(joined.contains("Client"));
        assert!(joined.contains("Server"));
        assert!(joined.contains("Next steps."));
    }

    #[test]
    fn test_render_markdown_table() {
        let md = "| Name | Status |\n|:---|---:|\n| Alice | Active |\n| Bob | Offline |\n";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
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
        assert!(joined.contains("Name"));
        assert!(joined.contains("Status"));
        assert!(joined.contains("Alice"));
        assert!(joined.contains("Active"));
        assert!(joined.contains("┌"));
        assert!(joined.contains("└"));
    }

    #[test]
    fn test_render_markdown_task_list() {
        let md = "- [ ] Unfinished task\n- [x] Finished task\n";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
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
        assert!(joined.contains("[ ]"));
        assert!(joined.contains("[✓]"));
    }

    #[test]
    fn test_render_markdown_ansi() {
        let md = "## Heading\n\nInline `code` snippet";
        let opts = MarkdownOptions::default();
        let ansi = render_markdown_ansi(md, &opts);
        assert!(ansi.contains("Heading"));
        assert!(ansi.contains("code"));
    }

    #[test]
    fn test_render_markdown_word_wrap() {
        let md = "This is a long paragraph of words that should definitely be wrapped onto multiple lines when max_width is constrained.";
        let opts = MarkdownOptions {
            max_width: Some(40),
            ..Default::default()
        };
        let text = render_markdown(md, &opts);
        assert!(
            text.lines.len() > 1,
            "Expected multiple lines due to wrapping"
        );
        for line in &text.lines {
            let w: usize = line
                .spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            assert!(w <= 40, "Line width {w} exceeded max_width 40");
        }
    }

    #[test]
    fn test_render_markdown_task_list_no_bullet_duplication() {
        let md = "- [ ] Unfinished task\n- [x] Finished task\n";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
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
            !joined.contains("• [ ]"),
            "Task list should not contain both bullet and checkbox"
        );
        assert!(
            !joined.contains("• [✓]"),
            "Task list should not contain both bullet and checkbox"
        );
        assert!(joined.contains("[ ] Unfinished task"));
        assert!(joined.contains("[✓] Finished task"));
    }

    #[test]
    fn test_render_markdown_image() {
        let md = "![Architecture Diagram](https://example.com/arch.png)";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
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
        assert!(joined.contains("🖼  [Architecture Diagram](https://example.com/arch.png)"));
    }

    #[test]
    fn test_render_markdown_blockquote_multiline() {
        let md = "> Quote paragraph 1\n>\n> Quote paragraph 2";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
        for line in &text.lines {
            let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(
                joined.starts_with('▌'),
                "Every blockquote line must have the vertical bar: {joined}"
            );
        }
    }

    #[test]
    fn test_render_markdown_no_duplicate_blank_lines() {
        let md = "- Item 1\n- Item 2\n\nParagraph after list";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
        let mut consecutive_blank = 0;
        for line in &text.lines {
            if is_line_blank(line) {
                consecutive_blank += 1;
                assert!(
                    consecutive_blank <= 1,
                    "Encountered more than one consecutive blank line"
                );
            } else {
                consecutive_blank = 0;
            }
        }
    }

    #[test]
    fn test_render_markdown_code_block_symmetric_borders() {
        let md = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
        let code_lines: Vec<&Line<'_>> = text.lines.iter().filter(|l| !is_line_blank(l)).collect();
        assert!(code_lines.len() >= 3);
        let widths: Vec<usize> = code_lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum()
            })
            .collect();
        let expected_w = widths[0];
        for (idx, w) in widths.iter().enumerate() {
            assert_eq!(
                *w, expected_w,
                "Line {idx} width ({w}) does not match top border width ({expected_w})"
            );
        }
    }

    #[test]
    fn test_extract_mermaid_blocks() {
        let md = r#"# Architecture

Here is the flow:

```mermaid
graph TD
    A --> B
```

And another:

~~~mermaid
sequenceDiagram
    Alice->>Bob: Hello
~~~
"#;
        let blocks = extract_mermaid_blocks(md);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, 4); // line index of opening fence
        assert!(blocks[0].1.contains("A --> B"));
        assert_eq!(blocks[1].0, 11);
        assert!(blocks[1].1.contains("Alice->>Bob"));
    }

    #[test]
    fn test_render_markdown_with_diagram_offsets() {
        let md = r#"# Document

Intro paragraph.

```mermaid
graph TD
    X --> Y
```

Outro paragraph.
"#;
        let opts = MarkdownOptions::default();
        let (text, offsets) = render_markdown_with_diagram_offsets(md, &opts);
        assert!(
            !offsets.is_empty(),
            "Offsets must detect the diagram box header"
        );
        assert!(
            offsets[0] < text.lines.len(),
            "Offset must point within rendered text"
        );
    }

    #[test]
    fn test_render_markdown_without_diagrams() {
        let md = r#"# Document

```mermaid
graph TD
    X --> Y
```
"#;
        let opts = MarkdownOptions {
            render_mermaid: false,
            ..Default::default()
        };
        let (text, offsets) = render_markdown_with_diagram_offsets(md, &opts);
        assert!(
            offsets.is_empty(),
            "Offsets must be empty when render_mermaid is false"
        );
        let rendered_str = format!("{text:?}");
        assert!(
            !rendered_str.contains("Mermaid Diagram"),
            "Diagram box title should not appear when render_mermaid is false"
        );
        assert!(
            rendered_str.contains("X --> Y"),
            "Original code block content should still be rendered"
        );
    }

    #[test]
    fn test_render_markdown_inline_diagrams() {
        let prev = std::env::var("MM_INLINE_DIAGRAMS").ok();
        unsafe {
            std::env::set_var("MM_INLINE_DIAGRAMS", "1");
        }

        let md = r#"# Document

```mermaid
graph LR
    A --> B
```
"#;
        let opts = MarkdownOptions {
            inline_diagrams: true,
            max_width: Some(80),
            ..Default::default()
        };
        let mut parser_opts = Options::empty();
        parser_opts.insert(Options::ENABLE_TABLES);
        parser_opts.insert(Options::ENABLE_TASKLISTS);
        parser_opts.insert(Options::ENABLE_STRIKETHROUGH);
        parser_opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
        let parser = Parser::new_ext(md, parser_opts);

        let mut renderer = MarkdownRenderer::new(&opts);
        renderer.render(parser);
        let (text, offsets, transmissions) = renderer.finish_with_transmissions();

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_DIAGRAMS", v),
                None => std::env::remove_var("MM_INLINE_DIAGRAMS"),
            }
        }

        assert!(!offsets.is_empty(), "Offsets must record the diagram position");
        assert!(!transmissions.is_empty(), "Transmissions must be emitted for inline diagram");
        assert!(transmissions[0].1.contains("U=1"), "Transmission must be Kitty Unicode placeholder format");

        let rendered_str = format!("{text:?}");
        println!("rendered_str = {rendered_str}");
        let found_in_spans = text
            .lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER)));
        assert!(
            !rendered_str.contains("inline diagram"),
            "Rendered text must NOT contain artificial inline diagram banner"
        );
        assert!(
            found_in_spans,
            "Rendered text spans must contain Kitty placeholder character"
        );
    }

    #[test]
    fn test_render_markdown_ansi_inline_diagrams() {
        let prev = std::env::var("MM_INLINE_DIAGRAMS").ok();
        unsafe {
            std::env::set_var("MM_INLINE_DIAGRAMS", "1");
        }

        let md = r#"```mermaid
graph TD
    X --> Y
```"#;
        let opts = MarkdownOptions {
            inline_diagrams: true,
            max_width: Some(70),
            ..Default::default()
        };
        let ansi = render_markdown_ansi(md, &opts);

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_DIAGRAMS", v),
                None => std::env::remove_var("MM_INLINE_DIAGRAMS"),
            }
        }

        assert!(
            ansi.contains("\u{10EEEE}"),
            "ANSI output must contain inline diagram placeholder"
        );
        assert!(
            !ansi.contains("inline diagram"),
            "ANSI output must NOT contain artificial inline diagram banner"
        );
        assert!(
            ansi.contains("_G"),
            "ANSI output must contain Kitty graphics escape sequence"
        );
    }

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_render_markdown_inline_local_image() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }

        // Create a temporary PNG image
        let temp_dir = std::env::temp_dir().join("mm_test_inline_img");
        let _ = std::fs::create_dir_all(&temp_dir);
        let img_path = temp_dir.join("test_img.png");

        let mut img_buf = image::RgbaImage::new(100, 100);
        for pixel in img_buf.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        img_buf.save(&img_path).unwrap();

        let md = format!("# Document\n\n![Test Image]({})\n\nAfter image", img_path.display());
        let opts = MarkdownOptions {
            inline_images: true,
            max_width: Some(60),
            ..Default::default()
        };

        let text = render_markdown(&md, &opts);

        let found_placeholder = text.lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER))
        });

        let ansi = render_markdown_ansi(&md, &opts);

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert!(
            found_placeholder,
            "Rendered markdown text must contain inline Kitty placeholder characters for local image"
        );
        assert!(
            ansi.contains("\u{10EEEE}"),
            "ANSI output must contain Unicode placeholder \\u{{10EEEE}}"
        );
        assert!(
            ansi.contains("_G"),
            "ANSI output must contain Kitty graphics escape sequence for transmitted image"
        );
        assert!(
            ansi.contains("U=1"),
            "Kitty graphics transmission must specify U=1 for Unicode placeholder mode"
        );
    }

    #[test]
    fn test_render_markdown_image_relative_path_and_lru_cache() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_image_placeholder_cache();

        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }

        let temp_dir = std::env::temp_dir().join("mm_test_rel_img");
        let sub_dir = temp_dir.join("assets");
        let _ = std::fs::create_dir_all(&sub_dir);
        let img_path = sub_dir.join("pic.png");

        let mut img_buf = image::RgbaImage::new(80, 80);
        for pixel in img_buf.pixels_mut() {
            *pixel = image::Rgba([0, 255, 0, 255]);
        }
        img_buf.save(&img_path).unwrap();

        let md_file = temp_dir.join("readme.md");
        std::fs::write(&md_file, "![My Pic](./assets/pic.png)").unwrap();

        let opts = MarkdownOptions {
            inline_images: true,
            max_width: Some(50),
            ..Default::default()
        };

        // 1. First render: loads from disk, caches in LRU
        let (text1, _) = render_markdown_file_with_diagram_offsets(&md_file, &opts).unwrap();
        let placeholder1 = text1.lines.iter().find(|l| {
            l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER))
        });
        assert!(placeholder1.is_some(), "Relative image must be found and rendered inline");

        // 2. Second render: must hit LRU cache with same image ID
        let (text2, _) = render_markdown_file_with_diagram_offsets(&md_file, &opts).unwrap();
        let placeholder2 = text2.lines.iter().find(|l| {
            l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER))
        });
        assert!(placeholder2.is_some(), "Second render must also succeed from LRU cache");

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_render_markdown_image_fallback_when_disabled_or_missing() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "0");
        }

        let md = "![Test Pic](some_image.png)";
        let opts = MarkdownOptions::default();
        let text = render_markdown(md, &opts);
        let joined: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            joined.contains("🖼  [Test Pic](some_image.png)"),
            "Must fall back to text representation when Kitty is disabled"
        );

        // When enabled but file is missing
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }
        let text_missing = render_markdown(md, &opts);
        let joined_missing: String = text_missing
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            joined_missing.contains("🖼  [Test Pic](some_image.png)"),
            "Must cleanly fall back to text representation when image file does not exist"
        );

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }
    }

    #[test]
    fn test_render_markdown_image_url_encoded_and_spaces() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }

        let temp_dir = std::env::temp_dir().join("mm_test_spaces_img");
        let _ = std::fs::create_dir_all(&temp_dir);
        let img_path = temp_dir.join("photo with spaces.png");

        let mut img_buf = image::RgbaImage::new(50, 50);
        for pixel in img_buf.pixels_mut() {
            *pixel = image::Rgba([0, 0, 255, 255]);
        }
        img_buf.save(&img_path).unwrap();

        // Reference using %20
        let md = "![Space Pic](photo%20with%20spaces.png)";
        let opts = MarkdownOptions {
            base_path: Some(temp_dir.join("doc.md")),
            inline_images: true,
            ..Default::default()
        };
        let text = render_markdown(md, &opts);
        let has_placeholder = text.lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER))
        });

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert!(
            has_placeholder,
            "Percent-encoded image URLs must resolve on disk and render as inline Kitty placeholders"
        );
    }

    #[test]
    fn test_render_markdown_image_data_uri() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }

        let mut img_buf = image::RgbaImage::new(10, 10);
        for pixel in img_buf.pixels_mut() {
            *pixel = image::Rgba([255, 255, 0, 255]);
        }
        let mut png_bytes = Vec::new();
        img_buf
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        let data_uri = format!("data:image/png;base64,{b64}");
        let md = format!("![Data URI]({data_uri})");

        let opts = MarkdownOptions {
            inline_images: true,
            ..Default::default()
        };
        let text = render_markdown(&md, &opts);
        let has_placeholder = text.lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER))
        });

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }

        assert!(
            has_placeholder,
            "Data URIs must decode and render inline Kitty placeholders"
        );
    }

    #[test]
    fn test_render_markdown_image_disabled_flag_independence() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        let prev_diag = std::env::var("MM_INLINE_DIAGRAMS").ok();
        unsafe {
            std::env::remove_var("MM_INLINE_IMAGES");
            std::env::set_var("MM_INLINE_DIAGRAMS", "1");
        }

        let temp_dir = std::env::temp_dir().join("mm_test_flag_indep");
        let _ = std::fs::create_dir_all(&temp_dir);
        let img_path = temp_dir.join("photo.png");

        let mut img_buf = image::RgbaImage::new(40, 40);
        for pixel in img_buf.pixels_mut() {
            *pixel = image::Rgba([100, 150, 200, 255]);
        }
        img_buf.save(&img_path).unwrap();

        let md = format!("![Photo]({})\n\n```mermaid\ngraph TD;\nA-->B;\n```", img_path.display());

        // inline_images = false, inline_diagrams = true
        let opts = MarkdownOptions {
            inline_images: false,
            inline_diagrams: true,
            ..Default::default()
        };

        let text = render_markdown(&md, &opts);
        let joined: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
            match prev_diag {
                Some(v) => std::env::set_var("MM_INLINE_DIAGRAMS", v),
                None => std::env::remove_var("MM_INLINE_DIAGRAMS"),
            }
        }
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Image MUST be fallback text
        assert!(
            joined.contains("🖼  [Photo]"),
            "When inline_images=false, images must NOT be rendered inline even if inline_diagrams=true"
        );
        // Diagram MUST still have placeholder lines
        let has_diagram_placeholder = text.lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER))
        });
        assert!(
            has_diagram_placeholder,
            "Diagram must still render inline when inline_diagrams=true"
        );
    }

    #[test]
    fn test_is_inline_images_enabled_env_semantics() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        let prev_diag = std::env::var("MM_INLINE_DIAGRAMS").ok();

        // 1. MM_INLINE_IMAGES=1 forces enabled regardless of opts
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
            std::env::set_var("MM_INLINE_DIAGRAMS", "0");
        }
        assert!(is_inline_images_enabled(false), "MM_INLINE_IMAGES=1 must force enable images even if opts.inline_images=false");
        assert!(is_inline_images_enabled(true), "MM_INLINE_IMAGES=1 must force enable images even if MM_INLINE_DIAGRAMS=0");

        // 2. MM_INLINE_IMAGES=0 forces disabled regardless of opts
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "0");
            std::env::set_var("MM_INLINE_DIAGRAMS", "1");
        }
        assert!(!is_inline_images_enabled(true), "MM_INLINE_IMAGES=0 must force disable images even if opts.inline_images=true and diagrams=1");

        // 3. MM_INLINE_DIAGRAMS=0 must NOT disable images if terminal supports it
        unsafe {
            std::env::remove_var("MM_INLINE_IMAGES");
            std::env::set_var("MM_INLINE_DIAGRAMS", "0");
        }
        if super::super::mermaid::is_kitty_terminal_supported() {
            assert!(is_inline_images_enabled(true), "MM_INLINE_DIAGRAMS=0 must not affect inline images when terminal is supported");
        }

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
            match prev_diag {
                Some(v) => std::env::set_var("MM_INLINE_DIAGRAMS", v),
                None => std::env::remove_var("MM_INLINE_DIAGRAMS"),
            }
        }
    }

    #[test]
    fn test_render_markdown_image_in_list_item_no_orphan_bullet() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }

        let temp_dir = std::env::temp_dir().join("mm_test_list_img");
        let _ = std::fs::create_dir_all(&temp_dir);
        let img_path = temp_dir.join("bullet_pic.png");

        let mut img_buf = image::RgbaImage::new(30, 30);
        for pixel in img_buf.pixels_mut() {
            *pixel = image::Rgba([12, 34, 56, 255]);
        }
        img_buf.save(&img_path).unwrap();

        let md = format!("- ![Bullet Pic]({})\n- Normal item", img_path.display());
        let opts = MarkdownOptions {
            inline_images: true,
            ..Default::default()
        };
        let text = render_markdown(&md, &opts);

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }
        let _ = std::fs::remove_dir_all(&temp_dir);

        // There should be NO line consisting solely of a bullet "• "
        for line in &text.lines {
            let line_str: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            let trimmed = line_str.trim();
            assert_ne!(trimmed, "•", "List item image must not produce an isolated orphan bullet line");
        }
    }

    #[test]
    fn test_render_markdown_image_file_scheme_and_relative_priority() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }

        let temp_dir = std::env::temp_dir().join("mm_test_file_scheme");
        let _ = std::fs::create_dir_all(&temp_dir);
        let img_path = temp_dir.join("photo.png");

        let mut img_buf = image::RgbaImage::new(20, 20);
        for pixel in img_buf.pixels_mut() {
            *pixel = image::Rgba([99, 88, 77, 255]);
        }
        img_buf.save(&img_path).unwrap();

        // 1. Test file://localhost/path
        let file_url = format!("file://localhost{}", img_path.display());
        let resolved = resolve_local_image_path(&file_url, None);
        assert_eq!(
            resolved,
            Some(std::fs::canonicalize(&img_path).unwrap()),
            "file://localhost/<abs_path> must resolve to absolute path with leading slash preserved"
        );

        // 2. Test relative priority over CWD
        let doc_dir = temp_dir.join("subdocs");
        let _ = std::fs::create_dir_all(&doc_dir);
        let doc_img = doc_dir.join("nested.png");
        let mut doc_img_buf = image::RgbaImage::new(20, 20);
        for pixel in doc_img_buf.pixels_mut() {
            *pixel = image::Rgba([1, 2, 3, 255]);
        }
        doc_img_buf.save(&doc_img).unwrap();

        let resolved_rel = resolve_local_image_path("nested.png", Some(&doc_dir.join("readme.md")));
        assert_eq!(
            resolved_rel,
            Some(std::fs::canonicalize(&doc_img).unwrap()),
            "Relative image path must resolve relative to document directory"
        );

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_render_markdown_image_svg_data_uri() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let prev = std::env::var("MM_INLINE_IMAGES").ok();
        unsafe {
            std::env::set_var("MM_INLINE_IMAGES", "1");
        }

        let svg_data = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="green"/></svg>"#;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(svg_data);
        let data_uri = format!("data:image/svg+xml;base64,{b64}");
        let md = format!("![SVG Data]({data_uri})");

        let opts = MarkdownOptions {
            inline_images: true,
            ..Default::default()
        };
        let text = render_markdown(&md, &opts);
        let has_placeholder = text.lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains(super::super::mermaid::PLACEHOLDER))
        });

        unsafe {
            match prev {
                Some(v) => std::env::set_var("MM_INLINE_IMAGES", v),
                None => std::env::remove_var("MM_INLINE_IMAGES"),
            }
        }

        assert!(
            has_placeholder,
            "SVG data URIs must decode and render inline Kitty placeholders"
        );
    }
}

