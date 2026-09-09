use cba::bring::split::split_on_nesting;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    config::RowConnectionStyle,
    ui::results::ResultsUI,
    utils::{string::substitute_escaped, text::expand_indents},
};

impl ResultsUI {
    pub fn make_status(&self, full_width: u16) -> Paragraph<'_> {
        let status_config = &self.status_config;
        let replacements = [
            ('r', self.index().to_string()),
            ('m', self.status.matched_count.to_string()),
            ('t', self.status.item_count.to_string()),
        ];

        // sub replacements into line
        let mut new_spans = Vec::new();

        if status_config.match_indent {
            new_spans.push(Span::raw(" ".repeat(self.indentation())));
        }

        for span in &self.status_template {
            let subbed = substitute_escaped(&span.content, &replacements);
            new_spans.push(Span::styled(subbed, span.style));
        }

        let substituted_line = Line::from(new_spans);

        // sub whitespace expansions
        let effective_width = match self.status_config.row_connection {
            RowConnectionStyle::Full => full_width,
            _ => self.width,
        } as usize;
        let expanded = expand_indents(substituted_line, r"\s", r"\S", effective_width)
            .style(status_config.style);

        Paragraph::new(expanded)
    }

    /// Returns just the substituted status spans as a `Line`, without width
    /// expansion or indentation — suitable for embedding inline in the input bar.
    pub fn status_line(&self) -> Line<'_> {
        let replacements = [
            ('r', self.index().to_string()),
            ('m', self.status.matched_count.to_string()),
            ('t', self.status.item_count.to_string()),
        ];

        let spans: Vec<Span<'_>> = self
            .status_template
            .iter()
            .map(|span| {
                let subbed = substitute_escaped(&span.content, &replacements);
                Span::styled(subbed, span.style)
            })
            .collect();

        Line::from(spans).style(self.status_config.style)
    }

    /// The style from the config overrides the Line style (but not the span styles).
    /// None restores the prompt defined in the config.
    pub fn set_status_line(&mut self, template: Option<Line<'static>>) {
        let status_config = &self.status_config;
        log::trace!("status line: {template:?}");

        self.status_template = template
            .unwrap_or(status_config.template.clone().into())
            .style(status_config.style)
            .into()
    }
}

pub struct StatusUI {}

impl StatusUI {
    pub fn parse_template_to_status_line(s: &str) -> Line<'static> {
        let parts = match split_on_nesting(&s, ['{', '}']) {
            Ok(x) => x,
            Err(n) => {
                if n > 0 {
                    log::error!("Encountered {} unclosed parentheses", n)
                } else {
                    log::error!("Extra closing parenthesis at index {}", -n)
                }
                return Line::from(s.to_string());
            }
        };

        let mut spans = Vec::new();
        let mut in_nested = !s.starts_with('{');
        let mut current_style = Style::default();

        for part in parts {
            in_nested = !in_nested;
            let content = part.as_str();

            if in_nested {
                let inner = &content[1..content.len() - 1];

                if inner.contains(':') {
                    // perform replacement style:content
                    spans.push(Self::span_from_template(inner));
                } else if inner.trim().eq_ignore_ascii_case("reset") {
                    current_style = Style::default();
                } else if let Some(style) = Self::parse_style(inner) {
                    current_style = style;
                } else {
                    spans.push(Span::styled(content.to_string(), current_style));
                }
            } else if !content.is_empty() {
                spans.push(Span::styled(content.to_string(), current_style));
            }
        }

        Line::from(spans)
    }

    /// Parses a comma-separated style specification (e.g. "red,bold" or "bg=blue,italic") into a `Style`.
    pub fn parse_style(style_part: &str) -> Option<Style> {
        use std::str::FromStr;

        let mut style = Style::default();
        let mut fg_set = false;
        let mut bg_set = false;
        let mut matched_any = false;

        for token in style_part.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }

            if token.eq_ignore_ascii_case("reset") {
                return Some(Style::default());
            }

            if let Some(bg_token) = token.strip_prefix("bg=").or_else(|| token.strip_prefix("bg:")) {
                if let Ok(color) = Color::from_str(bg_token) {
                    style = style.bg(color);
                    bg_set = true;
                    matched_any = true;
                    continue;
                }
            }

            if let Some(fg_token) = token.strip_prefix("fg=").or_else(|| token.strip_prefix("fg:")) {
                if let Ok(color) = Color::from_str(fg_token) {
                    style = style.fg(color);
                    fg_set = true;
                    matched_any = true;
                    continue;
                }
            }

            if !fg_set {
                if let Ok(color) = Color::from_str(token) {
                    style = style.fg(color);
                    fg_set = true;
                    matched_any = true;
                    continue;
                }
            }

            if !bg_set {
                if let Ok(color) = Color::from_str(token) {
                    style = style.bg(color);
                    bg_set = true;
                    matched_any = true;
                    continue;
                }
            }

            match token.to_lowercase().as_str() {
                "bold" => {
                    style = style.add_modifier(Modifier::BOLD);
                    matched_any = true;
                }
                "dim" => {
                    style = style.add_modifier(Modifier::DIM);
                    matched_any = true;
                }
                "italic" => {
                    style = style.add_modifier(Modifier::ITALIC);
                    matched_any = true;
                }
                "underlined" => {
                    style = style.add_modifier(Modifier::UNDERLINED);
                    matched_any = true;
                }
                "slow_blink" => {
                    style = style.add_modifier(Modifier::SLOW_BLINK);
                    matched_any = true;
                }
                "rapid_blink" => {
                    style = style.add_modifier(Modifier::RAPID_BLINK);
                    matched_any = true;
                }
                "reversed" => {
                    style = style.add_modifier(Modifier::REVERSED);
                    matched_any = true;
                }
                "hidden" => {
                    style = style.add_modifier(Modifier::HIDDEN);
                    matched_any = true;
                }
                "crossed_out" => {
                    style = style.add_modifier(Modifier::CROSSED_OUT);
                    matched_any = true;
                }
                _ => return None,
            };
        }

        if matched_any {
            Some(style)
        } else {
            None
        }
    }

    /// Converts a template string into a `Span` with colors and modifiers.
    ///
    /// The template string format is:
    /// ```text
    /// "style1,style2,...:text"
    /// ```
    /// - The **first valid color** token is used as foreground (fg).
    /// - The **second valid color** token is used as background (bg).
    /// - Remaining tokens are interpreted as **modifiers**: bold, dim, italic, underlined,
    ///   slow_blink, rapid_blink, reversed, hidden, crossed_out.
    /// - Empty tokens are ignored.
    /// - Unrecognized tokens are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use matchmaker::ui::StatusUI;
    /// StatusUI::span_from_template("red,bg=blue,bold,italic:Hello");
    /// StatusUI::span_from_template("green,,underlined:World");
    /// StatusUI::span_from_template(",,dim:OnlyDim");
    /// ```
    ///
    /// Returns a `Span` with the specified styles applied to the text.
    pub fn span_from_template(inner: &str) -> Span<'static> {
        let (style_part, text) = inner.split_once(':').unwrap_or(("", inner));
        let style = Self::parse_style(style_part).unwrap_or_default();
        Span::styled(text.to_string(), style)
    }
}
