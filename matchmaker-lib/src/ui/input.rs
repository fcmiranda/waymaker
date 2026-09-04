use std::ops::{Deref, DerefMut};

use ratatui::{
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::config::{QueryConfig, StyleSetting};

#[derive(Debug, Default, Clone)]
pub struct InputUI {
    pub cursor: usize, // index into graphemes, can = graphemes.len()
    pub input: String, // remember to call recompute_graphemes() after modifying directly
    /// (byte_index, width)
    pub graphemes: Vec<(usize, u16)>,
    pub before: usize, // index into graphemes of the first visible grapheme
    pub width: u16,    // only relevant to cursor scrolling
}

impl InputUI {
    pub fn new() -> Self {
        Self::default()
    }

    // -------- UTILS -----------
    pub fn recompute_graphemes(&mut self) {
        self.graphemes = self
            .input
            .grapheme_indices(true)
            .map(|(idx, g)| (idx, g.width() as u16))
            .collect();
    }

    pub fn byte_index(&self, grapheme_idx: usize) -> usize {
        self.graphemes
            .get(grapheme_idx)
            .map(|(idx, _)| *idx)
            .unwrap_or(self.input.len())
    }

    pub fn str_at_cursor(&self) -> &str {
        &self.input[..self.byte_index(self.cursor)]
    }

    // ---------- GETTERS ---------

    pub fn len(&self) -> usize {
        self.input.len()
    }
    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    /// grapheme index
    pub fn cursor(&self) -> u16 {
        self.cursor as u16
    }

    // ------------ SETTERS ---------------
    pub fn set(&mut self, input: impl Into<Option<String>>, cursor: u16) {
        if let Some(input) = input.into() {
            self.input = input;
            self.recompute_graphemes();
        }
        self.cursor = (cursor as usize).min(self.graphemes.len());
    }

    pub fn push_char(&mut self, c: char) {
        let byte_idx = self.byte_index(self.cursor);
        self.input.insert(byte_idx, c);
        self.recompute_graphemes();
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, content: &str) {
        let byte_idx = self.byte_index(self.cursor);
        self.input.insert_str(byte_idx, content);
        let added_graphemes = content.graphemes(true).count();
        self.recompute_graphemes();
        self.cursor += added_graphemes;
    }

    pub fn push_str(&mut self, content: &str) {
        self.input.push_str(content);
        self.recompute_graphemes();
        self.cursor = self.graphemes.len();
    }

    pub fn scroll_to_cursor(&mut self, padding: usize) {
        if self.width == 0 {
            return;
        }

        // when cursor moves behind or on start, display grapheme before cursor as the first visible,
        if self.before >= self.cursor {
            self.before = self.cursor.saturating_sub(padding);
            return;
        }

        // move start up
        loop {
            let visual_dist: u16 = self.graphemes
                [self.before..=(self.cursor + padding).min(self.graphemes.len().saturating_sub(1))]
                .iter()
                .map(|(_, w)| *w)
                .sum();

            // ensures visual_start..=cursor is displayed
            // Padding ensures the following element after cursor if present is displayed.
            if visual_dist <= self.width {
                break;
            }

            if self.before < self.cursor {
                self.before += 1;
            } else {
                // never move before over cursor
                break;
            }
        }
    }

    pub fn cancel(&mut self) {
        self.input.clear();
        self.graphemes.clear();
        self.cursor = 0;
        self.before = 0;
    }

    pub fn prepare_column_change(&mut self) {
        let trimmed = self.input.trim_end();
        if let Some(pos) = trimmed.rfind(' ') {
            let last_word = &trimmed[pos + 1..];
            if last_word.starts_with('%') {
                let bytes = trimmed[..pos].len();
                self.input.truncate(bytes);
            }
        } else if trimmed.starts_with('%') {
            self.input.clear();
        }

        if !self.input.is_empty() && !self.input.ends_with(' ') {
            self.input.push(' ');
        }
        self.recompute_graphemes();
        self.cursor = self.graphemes.len();
    }

    /// Set cursor to a visual offset relative to start position
    pub fn set_at_visual_offset(&mut self, visual_offset: u16) {
        let mut current_width = 0;
        let mut target_cursor = self.before;

        for (i, &(_, width)) in self.graphemes.iter().enumerate().skip(self.before) {
            if current_width + width > visual_offset {
                // If clicked on the right half of a character, move cursor after it
                if visual_offset - current_width > width / 2 {
                    target_cursor = i + 1;
                } else {
                    target_cursor = i;
                }
                break;
            }
            current_width += width;
            target_cursor = i + 1;
        }

        self.cursor = target_cursor;
    }

    // ---------- EDITING -------------
    pub fn forward_char(&mut self) {
        if self.cursor < self.graphemes.len() {
            self.cursor += 1;
        }
    }
    pub fn backward_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn forward_word(&mut self) {
        let mut in_word = false;
        while self.cursor < self.graphemes.len() {
            let byte_start = self.graphemes[self.cursor].0;
            let byte_end = self
                .graphemes
                .get(self.cursor + 1)
                .map(|(idx, _)| *idx)
                .unwrap_or(self.input.len());
            let g = &self.input[byte_start..byte_end];

            if g.chars().all(|c| c.is_whitespace()) {
                if in_word {
                    break;
                }
            } else {
                in_word = true;
            }
            self.cursor += 1;
        }
    }

    pub fn backward_word(&mut self) {
        let mut in_word = false;
        while self.cursor > 0 {
            let byte_start = self.graphemes[self.cursor - 1].0;
            let byte_end = self
                .graphemes
                .get(self.cursor)
                .map(|(idx, _)| *idx)
                .unwrap_or(self.input.len());
            let g = &self.input[byte_start..byte_end];

            if g.chars().all(|c| c.is_whitespace()) {
                if in_word {
                    break;
                }
            } else {
                in_word = true;
            }
            self.cursor -= 1;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor > 0 {
            let start = self.graphemes[self.cursor - 1].0;
            let end = self.byte_index(self.cursor);
            self.input.replace_range(start..end, "");
            self.recompute_graphemes();
            self.cursor -= 1;
        }
    }

    pub fn delete_word(&mut self) {
        let old_cursor = self.cursor;
        self.backward_word();
        let new_cursor = self.cursor;

        let start = self.byte_index(new_cursor);
        let end = self.byte_index(old_cursor);
        self.input.replace_range(start..end, "");
        self.recompute_graphemes();
    }

    pub fn delete_next(&mut self) {
        if self.cursor < self.graphemes.len() {
            let start = self.byte_index(self.cursor);
            let end = self.byte_index(self.cursor + 1);
            self.input.replace_range(start..end, "");
            self.recompute_graphemes();
        }
    }

    pub fn delete_next_word(&mut self) {
        let old_cursor = self.cursor;
        self.forward_word();
        let new_cursor = self.cursor;

        let start = self.byte_index(old_cursor);
        let end = self.byte_index(new_cursor);
        self.input.replace_range(start..end, "");
        self.recompute_graphemes();
        self.cursor = old_cursor;
    }

    pub fn delete_line_start(&mut self) {
        let end = self.byte_index(self.cursor);
        self.input.replace_range(0..end, "");
        self.recompute_graphemes();
        self.cursor = 0;
        self.before = 0;
    }

    pub fn delete_line_end(&mut self) {
        let start = self.byte_index(self.cursor);
        self.input.truncate(start);
        self.recompute_graphemes();
    }

    // ---------------------------------------
    // remember to call scroll_to_cursor beforehand

    pub fn render(&self) -> &str {
        let mut visible_width = 0;
        let mut end_idx = self.before;

        while end_idx < self.graphemes.len() {
            let g_width = self.graphemes[end_idx].1;
            if self.width != 0 && visible_width + g_width > self.width {
                break;
            }
            visible_width += g_width;
            end_idx += 1;
        }

        let start_byte = self.byte_index(self.before);
        let end_byte = self.byte_index(end_idx);
        let visible_input = &self.input[start_byte..end_byte];

        visible_input
    }

    pub fn cursor_rel_offset(&self) -> u16 {
        self.graphemes[self.before..self.cursor]
            .iter()
            .map(|(_, w)| *w)
            .sum()
    }
}

#[derive(Debug, Clone)]
pub struct QueryUI {
    pub state: InputUI,
    prompt: Line<'static>,
    pub custom_prompt: Option<Line<'static>>,
    pub config: QueryConfig,
    pub mode_index: usize,
}

impl Deref for QueryUI {
    type Target = InputUI;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl DerefMut for QueryUI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl QueryUI {
    pub fn new(config: QueryConfig) -> Self {
        let prompt = Line::styled(config.prompt.clone(), config.prompt_style);
        let mut ui = Self {
            state: InputUI::new(),
            prompt,
            custom_prompt: None,
            config,
            mode_index: 0,
        };

        if !ui.config.initial.is_empty() {
            ui.input = ui.config.initial.clone();
            ui.recompute_graphemes();
            ui.cursor = ui.graphemes.len();
        }

        ui
    }

    pub fn set_mode_index(&mut self, index: usize) {
        self.mode_index = index;
    }

    pub fn active_prompt(&self, focused: bool, nav_prompt: Option<&str>) -> Line<'static> {
        if let Some(ref custom) = self.custom_prompt {
            return custom.clone();
        }

        if focused || nav_prompt.is_none() || nav_prompt.unwrap().is_empty() {
            let (prompt_text, style) = match self.mode_index {
                1 => {
                    let text = self
                        .config
                        .frecency_prompt
                        .as_deref()
                        .unwrap_or("󱅤 ");
                    let st = if !self.config.frecency_prompt_style.is_empty() {
                        self.config.frecency_prompt_style
                    } else {
                        StyleSetting {
                            fg: Some(Color::Blue),
                            ..Default::default()
                        }
                    };
                    (text, st)
                }
                2 => {
                    let text = self
                        .config
                        .bookmarks_prompt
                        .as_deref()
                        .unwrap_or(" ");
                    let st = if !self.config.bookmarks_prompt_style.is_empty() {
                        self.config.bookmarks_prompt_style
                    } else {
                        StyleSetting {
                            fg: Some(Color::Yellow),
                            ..Default::default()
                        }
                    };
                    (text, st)
                }
                _ => {
                    let text = if focused {
                        self.config
                            .local_prompt
                            .as_deref()
                            .or(self.config.filter_prompt.as_deref())
                            .unwrap_or(&self.config.prompt)
                    } else {
                        self.config
                            .local_prompt
                            .as_deref()
                            .unwrap_or(&self.config.prompt)
                    };
                    let st = if !self.config.local_prompt_style.is_empty() {
                        self.config.local_prompt_style
                    } else if focused && !self.config.filter_prompt_style.is_empty() {
                        self.config.filter_prompt_style
                    } else {
                        self.config.prompt_style
                    };
                    (text, st)
                }
            };
            Line::styled(prompt_text.to_string(), style)
        } else {
            let np = nav_prompt.unwrap();
            Line::styled(np.to_string(), self.config.prompt_style)
        }
    }

    pub fn active_text_style(&self, focused: bool) -> Style {
        if focused && !self.config.filter_style.is_empty() {
            self.config.filter_style.r#override(Style::reset())
        } else {
            self.config.style.r#override(Style::reset())
        }
    }

    pub fn active_underline_style(&self, focused: bool) -> (bool, StyleSetting) {
        if focused {
            let active = self.config.filter_underline.unwrap_or(self.config.underline)
                || !self.config.filter_underline_style.is_empty()
                || !self.config.frecency_underline_style.is_empty()
                || !self.config.bookmarks_underline_style.is_empty()
                || !self.config.local_underline_style.is_empty();

            let style = match self.mode_index {
                1 => {
                    if !self.config.frecency_underline_style.is_empty() {
                        self.config.frecency_underline_style
                    } else if !self.config.frecency_prompt_style.is_empty() {
                        self.config.frecency_prompt_style
                    } else if !self.config.filter_underline_style.is_empty() {
                        self.config.filter_underline_style
                    } else {
                        StyleSetting {
                            fg: Some(Color::Blue),
                            ..Default::default()
                        }
                    }
                }
                2 => {
                    if !self.config.bookmarks_underline_style.is_empty() {
                        self.config.bookmarks_underline_style
                    } else if !self.config.bookmarks_prompt_style.is_empty() {
                        self.config.bookmarks_prompt_style
                    } else if !self.config.filter_underline_style.is_empty() {
                        self.config.filter_underline_style
                    } else {
                        StyleSetting {
                            fg: Some(Color::Yellow),
                            ..Default::default()
                        }
                    }
                }
                _ => {
                    if !self.config.local_underline_style.is_empty() {
                        self.config.local_underline_style
                    } else if !self.config.filter_underline_style.is_empty() {
                        self.config.filter_underline_style
                    } else {
                        self.config.underline_style
                    }
                }
            };
            (active, style)
        } else {
            let active = self.config.underline && self.config.filter_underline != Some(true);
            (active, self.config.underline_style)
        }
    }

    pub fn left(&self) -> u16 {
        self.config.border.left() + self.prompt.width() as u16
    }

    pub fn left_for_prompt(&self, prompt: &Line<'_>) -> u16 {
        self.config.border.left() + prompt.width() as u16
    }

    /// Given a rect the widget is rendered with, produce the absolute position the cursor is rendered at.
    pub fn cursor_offset(&self, rect: &Rect) -> Position {
        let top = self.config.border.top();
        Position::new(
            rect.x + self.left() + self.cursor_rel_offset(),
            rect.y + top,
        )
    }

    pub fn cursor_offset_for_prompt(&self, rect: &Rect, prompt: &Line<'_>) -> Position {
        let top = self.config.border.top();
        Position::new(
            rect.x + self.left_for_prompt(prompt) + self.cursor_rel_offset(),
            rect.y + top,
        )
    }

    // ------------ SETTERS ---------------
    pub fn update_width(&mut self, width: u16) {
        let text_width = width
            .saturating_sub(self.prompt.width() as u16)
            .saturating_sub(self.config.border.width());
        if self.width != text_width {
            self.width = text_width;
        }
    }

    pub fn scroll_to_cursor(&mut self) {
        let padding = self.config.scroll_padding as usize;
        self.state.scroll_to_cursor(padding);
    }

    // ---------------------------------------
    // remember to call scroll_to_cursor beforehand

    pub fn make_input(&self) -> Paragraph<'_> {
        self.make_input_focused(0, true, None)
    }

    pub fn make_input_focused(
        &self,
        area_width: u16,
        focused: bool,
        nav_prompt: Option<&str>,
    ) -> Paragraph<'_> {
        use unicode_width::UnicodeWidthStr;

        let mut line = self.active_prompt(focused, nav_prompt);
        line.push_span(Span::styled(
            self.state.render(),
            self.active_text_style(focused),
        ));

        let (should_underline, u_style) = self.active_underline_style(focused);

        if should_underline && area_width > 0 {
            let cur_w = line.spans.iter().map(|s| s.content.width()).sum::<usize>() as u16;
            let pad = area_width.saturating_sub(cur_w);
            if pad > 0 {
                line.push_span(Span::raw(" ".repeat(pad as usize)));
            }
            let u_fg = u_style.fg;
            for span in line.spans.iter_mut() {
                span.style = span.style.add_modifier(ratatui::style::Modifier::UNDERLINED);
                if let Some(fg) = u_fg {
                    span.style = span.style.underline_color(fg);
                }
            }
        }

        Paragraph::new(line).block(self.config.border.as_block())
    }

    /// Like `make_input` but right-aligns `right_label` (the inline status)
    /// inside the available area.  `area_width` is the rendered width of the
    /// input row (after border padding has been applied by the caller).
    pub fn make_input_with_status<'a>(
        &'a self,
        right_label: Line<'a>,
        area_width: u16,
    ) -> Paragraph<'a> {
        self.make_input_with_status_focused(right_label, area_width, true, None)
    }

    pub fn make_input_with_status_focused<'a>(
        &'a self,
        right_label: Line<'a>,
        area_width: u16,
        focused: bool,
        nav_prompt: Option<&str>,
    ) -> Paragraph<'a> {
        use unicode_width::UnicodeWidthStr;

        let mut line = self.active_prompt(focused, nav_prompt);
        line.push_span(Span::styled(
            self.state.render(),
            self.active_text_style(focused),
        ));

        // Calculate how much of area_width the left part already consumes.
        let left_width = line.spans.iter().map(|s| s.content.width()).sum::<usize>() as u16;

        // Width of the right label.
        let label_width = right_label
            .spans
            .iter()
            .map(|s| s.content.width())
            .sum::<usize>() as u16;

        // How many spaces we need to pad between left content and right label.
        let padding = area_width
            .saturating_sub(left_width)
            .saturating_sub(label_width);

        // Only append if there is room.
        if padding > 0 || left_width + label_width <= area_width {
            line.push_span(Span::raw(" ".repeat(padding as usize)));
            for span in right_label.spans {
                line.push_span(span);
            }
        }

        let (should_underline, u_style) = self.active_underline_style(focused);

        if should_underline && area_width > 0 {
            let cur_w = line.spans.iter().map(|s| s.content.width()).sum::<usize>() as u16;
            let pad = area_width.saturating_sub(cur_w);
            if pad > 0 {
                line.push_span(Span::raw(" ".repeat(pad as usize)));
            }
            let u_fg = u_style.fg;
            for span in line.spans.iter_mut() {
                span.style = span.style.add_modifier(ratatui::style::Modifier::UNDERLINED);
                if let Some(fg) = u_fg {
                    span.style = span.style.underline_color(fg);
                }
            }
        }

        Paragraph::new(line).block(self.config.border.as_block())
    }

    /// Set the input ui prefix. The prompt style from the config overrides the Line style (but not the span styles).
    /// None restores the prompt defined in the config.
    pub fn set_prompt(&mut self, template: Option<Line<'static>>) {
        if let Some(template) = template {
            let line = template.style(self.config.prompt_style);
            self.set_prompt_line(line);
        } else {
            self.custom_prompt = None;
            let default_prompt = Line::styled(self.config.prompt.clone(), self.config.prompt_style);
            let old_width = self.prompt.to_string().width();
            let new_width = default_prompt.to_string().width();

            if new_width > old_width {
                self.width = self.width.saturating_sub((new_width - old_width) as u16);
            } else if old_width > new_width {
                self.width += (old_width - new_width) as u16;
            }

            self.prompt = default_prompt;
        }
    }

    /// Set the input ui prefix directly.
    pub fn set_prompt_line(&mut self, prompt: Line<'static>) {
        let old_width = self.prompt.to_string().width();
        let new_width = prompt.to_string().width();

        if new_width > old_width {
            self.width = self.width.saturating_sub((new_width - old_width) as u16);
        } else if old_width > new_width {
            self.width += (old_width - new_width) as u16;
        }

        self.custom_prompt = Some(prompt.clone());
        self.prompt = prompt;
    }
}
