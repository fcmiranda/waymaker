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

struct MarkdownRenderer<'a> {
    opts: &'a MarkdownOptions,
    lines: Vec<Line<'static>>,
    current_line: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    heading_level: Option<HeadingLevel>,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    code_block: Option<CodeBlockState>,
    table: Option<TableState>,
    current_link_url: Option<String>,
    current_link_text: String,
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
            blockquote_depth: 0,
            code_block: None,
            table: None,
            current_link_url: None,
            current_link_text: String::new(),
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

    fn flush_line(&mut self) {
        if !self.current_line.is_empty() {
            let spans = std::mem::take(&mut self.current_line);
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
        if let Some(last) = self.lines.last()
            && !last.spans.is_empty()
        {
            self.lines.push(Line::from(""));
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
                if self.current_link_url.is_some() {
                    self.current_link_text.push_str(&text);
                }
                let style = self.current_style();
                self.current_line.push(Span::styled(text.into_string(), style));
            }
            Event::Code(code) => {
                let code_style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(35, 38, 46));
                self.current_line
                    .push(Span::styled(format!(" `{code}` "), code_style));
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    self.current_line.push(Span::styled(
                        "[✓] ",
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
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
                self.lines.push(Line::from(""));
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
                self.push_style(
                    Style::default()
                        .fg(color)
                        .add_modifier(Modifier::BOLD),
                );
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
                let indent = "  ".repeat(depth);
                if !indent.is_empty() {
                    self.current_line.push(Span::raw(indent));
                }
                if let Some(list_state) = self.list_stack.last_mut() {
                    if list_state.is_ordered {
                        let num = list_state.counter;
                        list_state.counter += 1;
                        self.current_line.push(Span::styled(
                            format!("{num}. "),
                            Style::default().fg(Color::Cyan),
                        ));
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
                        self.current_line.push(Span::styled(
                            bullet,
                            Style::default().fg(bullet_color),
                        ));
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
                    self.lines.push(Line::from(""));
                }
            }
            TagEnd::Item => {
                self.flush_line();
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
                self.lines.push(Line::from(""));
            }
            CodeBlockState::Code { lang, buffer } => {
                let border_style = Style::default().fg(Color::DarkGray);
                let lang_tag = if lang.is_empty() {
                    "code".to_string()
                } else {
                    lang
                };
                let header = format!("╭─ [{lang_tag}] ──────────────────────────────╮");
                self.lines.push(Line::from(Span::styled(header, border_style)));

                let raw_lines: Vec<&str> = buffer.lines().collect();
                let line_count = raw_lines.len();

                for (idx, line_str) in raw_lines.iter().enumerate() {
                    let mut spans = vec![Span::styled("│ ", border_style)];
                    if self.opts.show_line_numbers && line_count > 1 {
                        let num_str = format!("{:>3} │ ", idx + 1);
                        spans.push(Span::styled(num_str, Style::default().fg(Color::DarkGray)));
                    }
                    spans.push(Span::styled(
                        line_str.to_string(),
                        Style::default().fg(Color::LightYellow),
                    ));
                    self.lines.push(Line::from(spans));
                }

                let footer = "╰──────────────────────────────────────────────╯";
                self.lines.push(Line::from(Span::styled(footer, border_style)));
                self.lines.push(Line::from(""));
            }
        }
    }

    fn finish_table(&mut self, tbl: TableState) {
        if tbl.rows.is_empty() {
            return;
        }

        let num_cols = tbl.alignments.len().max(
            tbl.rows
                .iter()
                .map(|r| r.len())
                .max()
                .unwrap_or(0),
        );
        if num_cols == 0 {
            return;
        }

        // Calculate column widths
        let mut col_widths = vec![3usize; num_cols];
        for row in &tbl.rows {
            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx < num_cols {
                    col_widths[col_idx] = col_widths[col_idx].max(cell.width());
                }
            }
        }

        let border_style = Style::default().fg(Color::DarkGray);
        let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
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
        self.lines.push(Line::from(Span::styled(top_border, border_style)));

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

            if is_header {
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
        self.lines.push(Line::from(Span::styled(bot_border, border_style)));
        self.lines.push(Line::from(""));
    }

    fn finish(mut self) -> Text<'static> {
        self.flush_line();
        Text::from(self.lines)
    }
}

fn pad_cell(content: &str, width: usize, align: Alignment) -> String {
    let content_len = content.width();
    if content_len >= width {
        return content.to_string();
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
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
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
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
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
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
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
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
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
}
