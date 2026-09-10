use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::fs;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

use super::mermaid::{MermaidOptions, render_mermaid};
use super::text::text_to_ansi;

/// Options for Markdown rendering.
#[derive(Debug, Clone)]
pub struct MarkdownOptions {
    /// Maximum width / column budget for word wrapping and diagram layout.
    pub max_width: Option<usize>,
    /// Whether to render embedded ```mermaid ... ``` code blocks as diagrams.
    pub render_mermaid: bool,
    /// Whether to force ASCII-only characters in rendered Mermaid diagrams.
    pub mermaid_ascii: bool,
    /// Whether to show line numbers in standard code blocks.
    pub show_line_numbers: bool,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            max_width: None,
            render_mermaid: true,
            mermaid_ascii: false,
            show_line_numbers: false,
        }
    }
}

/// Render a Markdown source string to Ratatui `Text<'static>`.
pub fn render_markdown(src: &str, opts: &MarkdownOptions) -> Text<'static> {
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

/// Render a Markdown source string to an ANSI colored string for terminal stdout.
pub fn render_markdown_ansi(src: &str, opts: &MarkdownOptions) -> String {
    let text = render_markdown(src, opts);
    text_to_ansi(&text)
}

/// Read a Markdown file and render it as Ratatui `Text<'static>`.
pub fn render_markdown_file(path: &Path, opts: &MarkdownOptions) -> anyhow::Result<Text<'static>> {
    let content = fs::read_to_string(path)?;
    Ok(render_markdown(&content, opts))
}

/// Read a Markdown file and render it as an ANSI colored string for terminal stdout.
pub fn render_markdown_file_ansi(path: &Path, opts: &MarkdownOptions) -> anyhow::Result<String> {
    let content = fs::read_to_string(path)?;
    Ok(render_markdown_ansi(&content, opts))
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
            if depth > 0 {
                self.current_line.push(Span::raw("  ".repeat(depth)));
            }
            self.current_line.push(bullet);
        }
    }

    fn flush_line(&mut self) {
        self.ensure_pending_prefix();
        if !self.current_line.is_empty() {
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
                self.ensure_pending_prefix();
                if self.current_image_url.is_some() {
                    self.current_image_alt.push_str(&text);
                    return;
                }
                if self.current_link_url.is_some() {
                    self.current_link_text.push_str(&text);
                }
                let style = self.current_style();
                self.current_line
                    .push(Span::styled(text.into_string(), style));
            }
            Event::Code(code) => {
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
                self.ensure_blank_line();
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
                if lang.eq_ignore_ascii_case("mermaid") && self.opts.render_mermaid {
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
                self.ensure_pending_prefix();
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
                let mermaid_opts = MermaidOptions {
                    max_width: self.opts.max_width,
                    ascii: self.opts.mermaid_ascii,
                    show_box: true,
                    title: Some("Mermaid Diagram".to_string()),
                };
                let diagram_text = render_mermaid(&buffer, &mermaid_opts);
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

    fn finish(mut self) -> Text<'static> {
        self.flush_line();
        Text::from(self.lines)
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
}
