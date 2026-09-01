use ratatui::{
    layout::Alignment,
    style::{Modifier, Style},
    text::Span,
    widgets::{Row, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    SSS, Selection, Selector,
    config::{HorizontalSeparator, RowConnectionStyle},
    nucleo::Worker,
    render::Click,
    ui::results::{
        ResultsUI,
        icons::{extract_col0_name, insert_icon_span, maybe_append_symlink_target},
    },
    utils::{
        string::{fit_width, substitute_escaped},
        text::{clip_text_lines, prefix_span},
    },
};

impl ResultsUI {
    pub fn make_table<'a, T: SSS>(
        &mut self,
        active_column: usize,
        worker: &'a mut Worker<T>,
        selector: &mut Selector<T, impl Selection>,
        matcher: &mut nucleo::Matcher,
        click: &mut Click,
        nav_bar_style: Option<(ratatui::widgets::BorderType, ratatui::style::Style)>,
        freeze_snapshot: bool,
    ) -> Table<'a> {
        if let Some(pos) = self.initial_pos {
            let (_, status) = Worker::new_snapshot(&mut worker.nucleo);
            self.status = status;
            let total = self.status.matched_count;
            if total > 0 {
                let target = if pos >= 0 {
                    (pos as u32).min(total.saturating_sub(1))
                } else {
                    total.saturating_sub((-pos) as u32)
                };
                self.cursor_jump(target);
                if pos >= 0 {
                    if total > pos as u32 || !self.status.running {
                        self.initial_pos = None;
                    }
                } else if total >= (-pos) as u32 || !self.status.running {
                    self.initial_pos = None;
                }
            } else if !self.status.running {
                self.initial_pos = None;
            }
        }

        let cwd = std::env::current_dir().unwrap_or_default();
        let offset = self.bottom as u32;
        let end = self.bottom + self.height as u32;
        let as_cols = !self.config.stacked_columns;

        let get_border_char = |_is_first: bool,
                               _is_last: bool,
                               border_type: ratatui::widgets::BorderType|
         -> &'static str {
            match border_type {
                ratatui::widgets::BorderType::Plain | ratatui::widgets::BorderType::Rounded => "│",
                ratatui::widgets::BorderType::Double => "║",
                ratatui::widgets::BorderType::Thick => "█",
                ratatui::widgets::BorderType::QuadrantOutside => "▌",
                ratatui::widgets::BorderType::QuadrantInside => "▐",
                _ => "│",
            }
        };

        let current_nav_bar = self.config.current_nav_bar;
        let get_nav_bar_span = |is_first: bool,
                                is_last: bool,
                                is_current: bool|
         -> Option<ratatui::text::Span<'static>> {
            nav_bar_style.as_ref().map(|(border_type, style)| {
                let bt = if is_current {
                    current_nav_bar.unwrap_or(*border_type)
                } else {
                    *border_type
                };
                let ch = get_border_char(is_first, is_last, bt);
                ratatui::text::Span::styled(ch, *style)
            })
        };

        let multi_prefix = self.config.multi_prefix.clone();
        let nav_bar_multi_prefixes = nav_bar_style.as_ref().map(|(border_type, _)| {
            let rest: String = multi_prefix.chars().skip(1).collect();
            let make = |is_first: bool, is_last: bool| -> String {
                let border_char = get_border_char(is_first, is_last, *border_type);
                if multi_prefix.chars().next().is_some() {
                    format!("{}{}", border_char, rest)
                } else {
                    format!("{} ", border_char)
                }
            };
            (
                make(true, true),
                make(true, false),
                make(false, true),
                make(false, false),
            )
        });

        let get_dynamic_multi_prefix = |is_first: bool, is_last: bool| -> String {
            if let Some((both, first, last, mid)) = &nav_bar_multi_prefixes {
                if is_first && is_last {
                    both.clone()
                } else if is_first {
                    first.clone()
                } else if is_last {
                    last.clone()
                } else {
                    mid.clone()
                }
            } else {
                multi_prefix.clone()
            }
        };

        macro_rules! get_prefix {
            ($row:expr, $is_selected:expr, $idx:expr, $item:expr, $columns:expr, $is_first:expr, $is_last:expr) => {{
                let mut icon_name = String::new();
                let mut is_spinner = false;
                let mut spinner_col_idx = 0;

                if !$row.is_empty() && !self.config.spinner_prefix.is_empty() {
                    for (i, col_text) in $row.iter().enumerate() {
                        let text_content = extract_col0_name(col_text);
                        if text_content.contains(&self.config.spinner_prefix) {
                            is_spinner = true;
                            spinner_col_idx = i;
                            break;
                        }
                    }
                }

                if !$row.is_empty() {
                    icon_name = $columns[0].raw($item).into_owned();
                    if is_spinner && spinner_col_idx == 0 {
                        icon_name = icon_name.replace(&self.config.spinner_prefix, "");
                    }
                }

                if is_spinner {
                    if self.config.spinner_inline {
                        let frame = crate::spinner::Spinner::from_name(&self.config.spinner)
                            .current_frame();
                        crate::utils::text::replace_string_in_text(
                            &mut $row[spinner_col_idx],
                            &self.config.spinner_prefix,
                            &format!(" {frame}"),
                        );
                    } else {
                        crate::utils::text::strip_string_from_text(
                            &mut $row[spinner_col_idx],
                            &self.config.spinner_prefix,
                        );
                    }
                }
                let (is_yanked, is_cut) = if (!self.yank_paths.is_empty()
                    || !self.cut_paths.is_empty())
                    && !icon_name.is_empty()
                {
                    let path = std::path::Path::new(&icon_name);
                    let abs_path = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        cwd.join(path)
                    };
                    let abs_str = abs_path.to_string_lossy();
                    (
                        !self.yank_paths.is_empty() && self.yank_paths.contains(abs_str.as_ref()),
                        !self.cut_paths.is_empty() && self.cut_paths.contains(abs_str.as_ref()),
                    )
                } else {
                    (false, false)
                };

                let prefix = if is_spinner && !self.config.spinner_inline {
                    let frame =
                        crate::spinner::Spinner::from_name(&self.config.spinner).current_frame();
                    let f = format!("{frame} ");
                    crate::utils::string::fit_width(&f, self.config.multi_prefix.width())
                } else if $is_selected || is_yanked || is_cut {
                    get_dynamic_multi_prefix($is_first, $is_last)
                } else {
                    self.default_prefix($idx)
                };
                (
                    prefix,
                    icon_name,
                    is_spinner,
                    spinner_col_idx,
                    is_yanked,
                    is_cut,
                )
            }};
        }

        let width_limits = if as_cols {
            self.max_widths()
        } else {
            let default = self.width.saturating_sub(self.indentation() as u16);

            (0..worker.columns.len())
                .map(|i| {
                    if self.hidden_columns.get(i).copied().unwrap_or(false) {
                        0
                    } else {
                        default
                    }
                })
                .collect()
        };

        let columns = worker.columns.clone();
        let (mut results, mut widths, medians, status) = worker.results(
            offset,
            end,
            &width_limits,
            self.config.wrap,
            self.config.max_height,
            self.config.match_style.into(),
            matcher,
            self.config.autoscroll.clone(),
            self.hscroll,
            (
                if self.config.vscroll_current_only {
                    0
                } else {
                    self.vscroll
                },
                !as_cols,
            ),
            self.config.show_skipped,
            freeze_snapshot,
        );
        let results_len = results.len();

        // log::trace!(
        //     "len: {}, hscroll: {},  offset: {}, end: {}, limits: {:?}, medians: {:?}, last_widths: {:?}",
        //     results.len(),
        //     self.hscroll,
        //     offset,
        //     end,
        //     width_limits,
        //     medians,
        //     self.widths
        // );

        self.status = status.clone();
        self.medians = medians;
        if self.indentation() > 0 {
            if let Some(first_vis) = widths.iter().position(|&w| w != 0) {
                widths[first_vis] += self.indentation() as u16;
            } else if !widths.is_empty() {
                widths[0] += self.indentation() as u16;
            }
        }

        // Ensure visible column is at least wide enough for any group headers in results
        let max_group_width = results
            .iter()
            .filter_map(|(g, _, _)| match g.as_ref() {
                Some(crate::nucleo::GroupHeader::Named(s)) => {
                    Some(UnicodeWidthStr::width(&**s) as u16 + 2)
                }
                _ => None,
            })
            .max()
            .unwrap_or(0);
        if max_group_width > 0 {
            if let Some(first_vis) = widths.iter().position(|&w| w != 0) {
                widths[first_vis] = widths[first_vis].max(max_group_width);
            }
        }

        // When symlink targets are enabled, expand column 0 to use all
        // remaining horizontal space so the annotation has room to display.
        if self.config.symlink_target && !widths.is_empty() && widths[0] != 0 {
            let other_cols: u16 = widths[1..].iter().sum();
            let col0_max = self
                .width
                .saturating_sub(other_cols)
                .saturating_sub(self.column_spacing_width());
            widths[0] = widths[0].max(col0_max);
        }

        // should generally be true already, but act as a safeguard
        // for x in widths.iter_mut().zip(&self.hidden_columns) {
        //     if *x.1 {
        //         *x.0 = 0
        //     }
        // }
        let widths = widths;

        let match_count = status.matched_count;

        if match_count < self.bottom + self.cursor as u32 && !self.cursor_disabled {
            if !status.running && match_count > 0 {
                self.cursor_jump(match_count);
            }
        } else {
            self.cursor = self.cursor.min(results.len().saturating_sub(1) as u16);
        }

        let tier_sep_style = self.config.tier_separator_style;
        let is_underline_mode = self.config.tier_separator == HorizontalSeparator::Underline;
        let apply_tier_underline =
            |t: &mut ratatui::text::Text<'_>, target_w: usize, skip_spans: usize| {
                if let Some(last_line) = t.lines.last_mut() {
                    let cur_w = last_line.width();
                    if cur_w < target_w {
                        let pad_style = if let Some(fg) = tier_sep_style.fg {
                            Style::default()
                                .fg(fg)
                                .underline_color(fg)
                                .add_modifier(Modifier::UNDERLINED)
                        } else {
                            Style::default().add_modifier(Modifier::UNDERLINED)
                        };
                        last_line
                            .spans
                            .push(Span::styled(" ".repeat(target_w - cur_w), pad_style));
                    }
                    for span in last_line.spans.iter_mut().skip(skip_spans) {
                        span.style = span.style.add_modifier(Modifier::UNDERLINED);
                        if let Some(fg) = tier_sep_style.fg {
                            span.style = span.style.underline_color(fg);
                        }
                    }
                }
            };

        let mut rows = vec![];
        let mut total_height = 0;

        if results.is_empty() {
            return Table::new(rows, widths);
        }

        let height_of = |t: &(
            Option<crate::nucleo::GroupHeader>,
            Vec<ratatui::text::Text<'a>>,
            _,
        )| {
            let group_h = match &t.0 {
                Some(crate::nucleo::GroupHeader::Named(_)) => 1,
                Some(crate::nucleo::GroupHeader::TierSeparator) => {
                    if !matches!(
                        self.config.tier_separator,
                        HorizontalSeparator::None | HorizontalSeparator::Underline
                    ) {
                        1
                    } else {
                        0
                    }
                }
                None => 0,
            };
            group_h
                + self._hr()
                + if as_cols {
                    t.1.iter()
                        .map(|t| t.height() as u16)
                        .max()
                        .unwrap_or_default()
                } else {
                    t.1.iter().map(|t| t.height() as u16).sum::<u16>()
                }
        };

        let effective_active_col = if widths.get(active_column).copied().unwrap_or(0) == 0 {
            widths.iter().position(|&w| w != 0).unwrap_or(active_column)
        } else {
            active_column
        };

        let style_text = |mut t: ratatui::text::Text<'a>, x: usize, is_current_row: bool| {
            let is_active_col = effective_active_col == x;
            match self.config.row_connection {
                RowConnectionStyle::Disjoint => {
                    if is_active_col {
                        t = t.style(if is_current_row {
                            self.config.current_style
                        } else {
                            self.config.style
                        });
                    } else {
                        t = t.style(if is_current_row {
                            self.config.inactive_current_style
                        } else {
                            self.config.inactive_style
                        });
                    }
                }
                RowConnectionStyle::Capped => {
                    if is_active_col {
                        t = t.style(if is_current_row {
                            self.config.current_style
                        } else {
                            self.config.style
                        });
                    }
                }
                RowConnectionStyle::Full => {}
            }
            t
        };

        // log::trace!("results initial: {}, {}, {}, {}, {}", self.bottom, self.cursor, total_height, self.height, results.len());
        let h_at_cursor = height_of(&results[self.cursor as usize]);
        let h_after_cursor = results[self.cursor as usize + 1..]
            .iter()
            .map(height_of)
            .sum();
        let h_to_cursor = results[0..self.cursor as usize]
            .iter()
            .map(height_of)
            .sum::<u16>();
        let cursor_end_should_lte = self.height - self.scroll_padding().min(h_after_cursor);
        // let cursor_start_should_gt = self.scroll_padding().min(h_to_cursor);

        // log::trace!(
        //     "Computed heights: {}, {h_at_cursor}, {h_to_cursor}, {h_after_cursor}, {cursor_end_should_lte}",
        //     self.cursor
        // );

        // begin adjustment
        let mut start_index = 0; // the index in results of the first complete item
        let is_current_row = false;
        if h_at_cursor >= cursor_end_should_lte {
            start_index = self.cursor;
            self.bottom += self.cursor as u32;
            self.cursor = 0;
            self.cursor_above = 0;
            self.bottom_clip = None;
        } else
        // increase the bottom index so that cursor_should_above is maintained
        if let h_to_cursor_end = h_to_cursor + h_at_cursor
            && h_to_cursor_end > cursor_end_should_lte
        {
            let mut trunc_height = h_to_cursor_end - cursor_end_should_lte;
            // note that there is a funny side effect that scrolling up near the bottom can scroll up a bit, but it seems fine to me

            for r in results[start_index as usize..self.cursor as usize].iter_mut() {
                let h = height_of(r);
                let (_, row, item) = r;
                start_index += 1; // we always skip at least the first item

                if trunc_height < h {
                    let mut remaining_height = h - trunc_height;
                    let is_selected = selector.contains(item);
                    let is_first = rows.is_empty();
                    let is_last = (self.height <= total_height + remaining_height)
                        || (start_index as usize >= results_len);
                    let nav_bar_span = get_nav_bar_span(is_first, is_last, is_current_row);
                    let (prefix, icon_name, is_spinner, spinner_col_idx, is_yanked, is_cut) =
                        get_prefix!(row, is_selected, 0, item, columns, is_first, is_last);

                    total_height += remaining_height;

                    // log::debug!("r: {remaining_height}");
                    if as_cols {
                        if remaining_height < h - self._hr() {
                            for (_, t) in
                                row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0)
                            {
                                clip_text_lines(t, remaining_height, !self.reverse());
                            }
                        }

                        let first_visible = widths.iter().position(|&w| w != 0).unwrap_or(0);
                        let target_prefix_col = if is_spinner
                            && widths.get(spinner_col_idx).copied().unwrap_or(0) != 0
                        {
                            spinner_col_idx
                        } else {
                            first_visible
                        };

                        let last_visible = widths
                            .iter()
                            .enumerate()
                            .rev()
                            .find_map(|(i, w)| (*w != 0).then_some(i));

                        let mut row_texts: Vec<_> = row
                            .iter()
                            .take(last_visible.map(|x| x + 1).unwrap_or(0))
                            .cloned()
                            .enumerate()
                            .map(|(x, mut t)| {
                                t = style_text(t, x, is_current_row);
                                if x == target_prefix_col {
                                    prefix_span(
                                        &mut t,
                                        prefix.clone(),
                                        self.active_prefix_style(
                                            &icon_name,
                                            is_selected,
                                            is_spinner,
                                            &cwd,
                                        ),
                                        self.inactive_prefix_style(
                                            &icon_name,
                                            is_selected,
                                            is_spinner,
                                            &cwd,
                                        ),
                                        is_current_row,
                                        if !is_selected && !is_yanked && !is_cut {
                                            nav_bar_span.clone()
                                        } else {
                                            None
                                        },
                                        self.config.current_style,
                                        self.config.current_nav_bar_style,
                                    );
                                    if self.config.icons {
                                        insert_icon_span(
                                            &mut t,
                                            &icon_name,
                                            !is_selected
                                                && !is_yanked
                                                && !is_cut
                                                && nav_bar_span.is_some(),
                                            is_current_row,
                                            self.config.uncolor_current_icon,
                                            self.config.invert_current_icon,
                                            self.config.current_icon_style,
                                        );
                                    }
                                    if self.config.symlink_target {
                                        maybe_append_symlink_target(
                                            &mut t,
                                            &icon_name,
                                            self.config.symlink_target_style.into(),
                                            widths[target_prefix_col],
                                        );
                                    }
                                }
                                t
                            })
                            .collect();

                        if self.config.right_align_last && row_texts.len() > 1 {
                            row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                        }

                        let text_h = row_texts
                            .iter()
                            .map(|t| t.lines.len() as u16)
                            .max()
                            .unwrap_or(1);
                        let row = Row::new(row_texts).height(text_h.min(remaining_height));
                        let row = if is_selected {
                            row.style(Style::from(self.config.selected_style))
                        } else {
                            row
                        };
                        rows.push(row);
                    } else {
                        let col_count = row.len();
                        let mut push = vec![];

                        for (rev_i, mut col) in row.into_iter().rev().enumerate() {
                            let col_idx = col_count.saturating_sub(1 + rev_i);
                            let mut height = col.height() as u16;
                            if remaining_height == 0 {
                                break;
                            } else if remaining_height < height {
                                clip_text_lines(&mut col, remaining_height, !self.reverse());
                                height = remaining_height;
                            }
                            remaining_height -= height;

                            prefix_span(
                                &mut col,
                                prefix.clone(),
                                self.active_prefix_style(&icon_name, is_selected, is_spinner, &cwd),
                                self.inactive_prefix_style(
                                    &icon_name,
                                    is_selected,
                                    is_spinner,
                                    &cwd,
                                ),
                                is_current_row,
                                if !is_selected && !is_yanked && !is_cut {
                                    nav_bar_span.clone()
                                } else {
                                    None
                                },
                                self.config.current_style,
                                self.config.current_nav_bar_style,
                            );
                            if self.config.icons && col_idx == 0 {
                                insert_icon_span(
                                    &mut col,
                                    &icon_name,
                                    !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some(),
                                    is_current_row,
                                    self.config.uncolor_current_icon,
                                    self.config.invert_current_icon,
                                    self.config.current_icon_style,
                                );
                            }
                            if self.config.symlink_target && col_idx == 0 {
                                maybe_append_symlink_target(
                                    col,
                                    &icon_name,
                                    self.config.symlink_target_style.into(),
                                    self.width,
                                );
                            }

                            let row = Row::new(vec![col.clone()]).height(height);
                            let row = if is_selected {
                                row.style(Style::from(self.config.selected_style))
                            } else {
                                row
                            };
                            push.push(row);
                        }
                        rows.extend(push.into_iter().rev());
                    }

                    self.bottom += start_index as u32 - 1;
                    self.cursor -= start_index - 1;
                    self.bottom_clip = Some(remaining_height);
                    break;
                } else if trunc_height == h {
                    self.bottom += start_index as u32;
                    self.cursor -= start_index;
                    self.bottom_clip = None;
                    break;
                }

                trunc_height -= h;
            }
        } else if let Some(mut remaining_height) = self.bottom_clip {
            start_index += 1;
            let is_topside_tier_underlined = is_underline_mode
                && ((start_index as usize) < results_len
                    && matches!(
                        results.get(start_index as usize),
                        Some((Some(crate::nucleo::GroupHeader::TierSeparator), _, _))
                    ));
            // same as above
            let h = height_of(&results[0]);
            let (_, row, item) = &mut results[0];
            let is_selected = selector.contains(item);
            let is_first = rows.is_empty();
            let is_last = (self.height <= total_height + remaining_height)
                || (start_index as usize >= results_len);
            let nav_bar_span = get_nav_bar_span(is_first, is_last, is_current_row);
            let (prefix, icon_name, is_spinner, spinner_col_idx, is_yanked, is_cut) =
                get_prefix!(row, is_selected, 0, item, columns, is_first, is_last);

            total_height += remaining_height;

            if as_cols {
                if self._hr() + remaining_height != h {
                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        clip_text_lines(t, remaining_height, !self.reverse());
                    }
                }

                let first_visible = widths.iter().position(|&w| w != 0).unwrap_or(0);
                let target_prefix_col =
                    if is_spinner && widths.get(spinner_col_idx).copied().unwrap_or(0) != 0 {
                        spinner_col_idx
                    } else {
                        first_visible
                    };

                let last_visible = widths
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, w)| (*w != 0).then_some(i));

                let mut row_texts: Vec<_> = row
                    .iter()
                    .take(last_visible.map(|x| x + 1).unwrap_or(0))
                    .cloned()
                    .enumerate()
                    .map(|(x, mut t)| {
                        t = style_text(t, x, is_current_row);
                        if x == target_prefix_col {
                            prefix_span(
                                &mut t,
                                prefix.clone(),
                                self.active_prefix_style(&icon_name, is_selected, is_spinner, &cwd),
                                self.inactive_prefix_style(
                                    &icon_name,
                                    is_selected && !is_current_row,
                                    is_spinner,
                                    &cwd,
                                ),
                                is_current_row,
                                if !is_selected && !is_yanked && !is_cut {
                                    nav_bar_span.clone()
                                } else {
                                    None
                                },
                                self.config.current_style,
                                self.config.current_nav_bar_style,
                            );
                            if self.config.icons {
                                insert_icon_span(
                                    &mut t,
                                    &icon_name,
                                    !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some(),
                                    is_current_row,
                                    self.config.uncolor_current_icon,
                                    self.config.invert_current_icon,
                                    self.config.current_icon_style,
                                );
                            }
                            if self.config.symlink_target {
                                maybe_append_symlink_target(
                                    &mut t,
                                    &icon_name,
                                    self.config.symlink_target_style.into(),
                                    widths[target_prefix_col],
                                );
                            }
                        }
                        t
                    })
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                if is_topside_tier_underlined {
                    let total_allocated: u16 = widths.iter().sum();
                    let total_available = self.width.saturating_sub(self.column_spacing_width());
                    let surplus = total_available.saturating_sub(total_allocated);
                    let num_cols = row_texts.len();
                    let has_nav = !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some();
                    let prefix_skip = if has_nav { 2 } else { 1 };
                    for (col_idx, t) in row_texts.iter_mut().enumerate() {
                        let is_last_col = col_idx == num_cols.saturating_sub(1);
                        let base_w = widths.get(col_idx).copied().unwrap_or(0);
                        let target_w = if is_last_col {
                            base_w + surplus
                        } else {
                            base_w
                        } as usize;
                        let skip = if col_idx == target_prefix_col {
                            prefix_skip
                        } else {
                            0
                        };
                        apply_tier_underline(t, target_w, skip);
                    }
                }

                let text_h = row_texts
                    .iter()
                    .map(|t| t.lines.len() as u16)
                    .max()
                    .unwrap_or(1);
                let row = Row::new(row_texts).height(text_h.min(remaining_height));
                let row = if is_selected && !is_current_row {
                    row.style(Style::from(self.config.selected_style))
                } else {
                    row
                };
                rows.push(row);
            } else {
                let col_count = row.len();
                let mut push = vec![];

                for (rev_i, mut col) in row.into_iter().rev().enumerate() {
                    let col_idx = col_count.saturating_sub(1 + rev_i);
                    let mut height = col.height() as u16;
                    if remaining_height == 0 {
                        break;
                    } else if remaining_height < height {
                        clip_text_lines(&mut col, remaining_height, !self.reverse());
                        height = remaining_height;
                    }
                    remaining_height -= height;

                    prefix_span(
                        &mut col,
                        prefix.clone(),
                        self.active_prefix_style(&icon_name, is_selected, is_spinner, &cwd),
                        self.inactive_prefix_style(&icon_name, is_selected, is_spinner, &cwd),
                        is_current_row,
                        if !is_selected && !is_yanked && !is_cut {
                            nav_bar_span.clone()
                        } else {
                            None
                        },
                        self.config.current_style,
                        self.config.current_nav_bar_style,
                    );
                    if self.config.icons && col_idx == 0 {
                        insert_icon_span(
                            &mut col,
                            &icon_name,
                            !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some(),
                            is_current_row,
                            self.config.uncolor_current_icon,
                            self.config.invert_current_icon,
                            self.config.current_icon_style,
                        );
                    }
                    if self.config.symlink_target && col_idx == 0 {
                        maybe_append_symlink_target(
                            col,
                            &icon_name,
                            self.config.symlink_target_style.into(),
                            self.width,
                        );
                    }

                    if is_topside_tier_underlined {
                        let target_w = self.width as usize;
                        let has_nav =
                            !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some();
                        let prefix_skip = if has_nav { 2 } else { 1 };
                        let skip = if col_idx == 0 { prefix_skip } else { 0 };
                        apply_tier_underline(&mut col, target_w, skip);
                    }

                    let row = Row::new(vec![col.clone()]).height(height);
                    let row = if is_selected && !is_current_row {
                        row.style(Style::from(self.config.selected_style))
                    } else {
                        row
                    };
                    push.push(row);
                }
                rows.extend(push.into_iter().rev());
            }
        }

        // topside padding is not self-correcting, and can only do its best to stay at #padding lines without obscuring cursor on cursor movement events.
        let mut remaining_height = self.height.saturating_sub(total_height);

        let mut i = self.bottom_clip.is_some() as usize;

        let mut drain_iter = results.drain(start_index as usize..).peekable();
        while let Some((group, mut row, item)) = drain_iter.next() {
            // note that the index changes *next* frame
            if let Click::ResultPos(c) = click {
                let c = if self.reverse() {
                    self.height.saturating_sub(*c).saturating_sub(1)
                } else {
                    *c
                };
                if self.height - remaining_height > c {
                    let idx = self.bottom as u32 + i as u32 - 1;
                    log::debug!(
                        "Mapped click position to index: {c} -> {idx} with remaining {remaining_height}",
                    );
                    *click = Click::ResultIdx(idx);
                }
            }

            // insert group header or tier separator
            if let Some(ref group) = group {
                if remaining_height > 0 {
                    let is_first = rows.is_empty();
                    let is_last = remaining_height <= 1;
                    let nav_bar_span = get_nav_bar_span(is_first, is_last, false);

                    let row_opt = match group {
                        crate::nucleo::GroupHeader::Named(group_name) => {
                            let group_style: Style = self.config.group_header_style.into();
                            let mut line_spans = vec![];
                            if let Some(nav_span) = nav_bar_span {
                                line_spans.push(nav_span);
                            }
                            line_spans.push(Span::raw(" "));
                            line_spans.push(Span::styled(group_name.to_string(), group_style));

                            let line = ratatui::text::Line::from(line_spans);
                            if as_cols {
                                let first_visible =
                                    widths.iter().position(|&w| w != 0).unwrap_or(0);
                                let mut cells = vec![];
                                for i in 0..widths.len() {
                                    if i == first_visible {
                                        cells.push(ratatui::widgets::Cell::from(line.clone()));
                                    } else {
                                        cells.push(ratatui::widgets::Cell::from(""));
                                    }
                                }
                                Some(Row::new(cells).height(1))
                            } else {
                                Some(Row::new(vec![line]).height(1))
                            }
                        }
                        crate::nucleo::GroupHeader::TierSeparator => {
                            let tier_sep = self.config.tier_separator;
                            if matches!(
                                tier_sep,
                                HorizontalSeparator::None | HorizontalSeparator::Underline
                            ) {
                                None
                            } else {
                                let sep_char = tier_sep.as_str();
                                let tier_style: Style = self.config.tier_separator_style.into();
                                let total_w = self.width as usize;

                                if as_cols {
                                    let mut cells = vec![];
                                    let first_visible =
                                        widths.iter().position(|&w| w != 0).unwrap_or(0);
                                    for (col_i, &w) in widths.iter().enumerate() {
                                        if w == 0 {
                                            cells.push(ratatui::widgets::Cell::from(""));
                                            continue;
                                        }
                                        let mut spans = vec![];
                                        let mut fill_w = w as usize;
                                        if col_i == first_visible {
                                            if let Some(nav_span) = nav_bar_span.clone() {
                                                fill_w = fill_w.saturating_sub(nav_span.width());
                                                spans.push(nav_span);
                                            }
                                        }
                                        let line_str = sep_char.repeat(fill_w);
                                        spans.push(Span::styled(line_str, tier_style));
                                        cells.push(ratatui::widgets::Cell::from(
                                            ratatui::text::Line::from(spans),
                                        ));
                                    }
                                    Some(Row::new(cells).height(1))
                                } else {
                                    let mut spans = vec![];
                                    let mut fill_w = total_w;
                                    if let Some(nav_span) = nav_bar_span {
                                        fill_w = fill_w.saturating_sub(nav_span.width());
                                        spans.push(nav_span);
                                    }
                                    let line_str = sep_char.repeat(fill_w);
                                    spans.push(Span::styled(line_str, tier_style));
                                    Some(Row::new(vec![ratatui::text::Line::from(spans)]).height(1))
                                }
                            }
                        }
                    };

                    if let Some(row) = row_opt {
                        rows.push(row);
                        remaining_height = remaining_height.saturating_sub(1);
                    }
                }
            }
            if remaining_height == 0 {
                break;
            }

            // insert hr
            if let Some(hr) = self.hr()
                && remaining_height > 0
            {
                rows.push(hr);
                remaining_height -= self._hr();
            }
            if remaining_height == 0 {
                break;
            }

            if self.is_current(i) {
                self.cursor_above = self.height - remaining_height;
            }

            // determine prefix
            let is_selected = selector.contains(item);
            let is_first = rows.is_empty();
            let is_last_in_results = drain_iter.peek().is_none();
            let h = if as_cols {
                row.iter()
                    .map(|t| t.height() as u16)
                    .max()
                    .unwrap_or_default()
            } else {
                row.iter().map(|t| t.height() as u16).sum::<u16>()
            };
            let is_last = is_last_in_results || (remaining_height <= h);
            let is_current_row = self.is_current(i);
            let nav_bar_span = get_nav_bar_span(is_first, is_last, is_current_row);
            let (prefix, icon_name_hz, is_spinner, spinner_col_idx, is_yanked, is_cut) =
                get_prefix!(row, is_selected, i, item, columns, is_first, is_last);

            if as_cols {
                // scroll down
                if self.is_current(i) && self.config.vscroll_current_only && self.vscroll > 0 {
                    for (x, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        if self.col.is_none() || self.col() == Some(x) {
                            let scroll = self.vscroll as usize;

                            if scroll < t.lines.len() {
                                t.lines = t.lines.split_off(scroll);
                            } else {
                                t.lines.clear();
                            }
                        }
                    }
                }

                let mut height = row
                    .iter()
                    .map(|t| t.height() as u16)
                    .max()
                    .unwrap_or_default();

                if remaining_height < height {
                    height = remaining_height;

                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        clip_text_lines(t, height, self.reverse());
                    }
                }
                remaining_height -= height;

                // same as above
                let first_visible = widths.iter().position(|&w| w != 0).unwrap_or(0);
                let target_prefix_col =
                    if is_spinner && widths.get(spinner_col_idx).copied().unwrap_or(0) != 0 {
                        spinner_col_idx
                    } else {
                        first_visible
                    };

                let last_visible = widths
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, w)| (*w != 0).then_some(i));

                let mut row_texts: Vec<_> = row
                    .iter()
                    .take(last_visible.map(|x| x + 1).unwrap_or(0))
                    .cloned()
                    // highlight
                    .enumerate()
                    .map(|(x, mut t)| {
                        t = style_text(t, x, self.is_current(i));

                        // prefix after hscroll
                        if x == target_prefix_col {
                            prefix_span(
                                &mut t,
                                prefix.clone(),
                                self.active_prefix_style(
                                    &icon_name_hz,
                                    is_selected,
                                    is_spinner,
                                    &cwd,
                                ),
                                self.inactive_prefix_style(
                                    &icon_name_hz,
                                    is_selected && !is_current_row,
                                    is_spinner,
                                    &cwd,
                                ),
                                is_current_row,
                                if !is_selected && !is_yanked && !is_cut {
                                    nav_bar_span.clone()
                                } else {
                                    None
                                },
                                self.config.current_style,
                                self.config.current_nav_bar_style,
                            );
                            if self.config.icons {
                                insert_icon_span(
                                    &mut t,
                                    &icon_name_hz,
                                    !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some(),
                                    is_current_row,
                                    self.config.uncolor_current_icon,
                                    self.config.invert_current_icon,
                                    self.config.current_icon_style,
                                );
                            }
                            if self.config.symlink_target {
                                maybe_append_symlink_target(
                                    &mut t,
                                    &icon_name_hz,
                                    self.config.symlink_target_style.into(),
                                    widths[target_prefix_col],
                                );
                            }
                        };
                        t
                    })
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                if is_underline_mode
                    && matches!(
                        drain_iter.peek(),
                        Some((Some(crate::nucleo::GroupHeader::TierSeparator), _, _))
                    )
                {
                    let total_allocated: u16 = widths.iter().sum();
                    let total_available = self.width.saturating_sub(self.column_spacing_width());
                    let surplus = total_available.saturating_sub(total_allocated);
                    let num_cols = row_texts.len();
                    let has_nav = !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some();
                    let prefix_skip = if has_nav { 2 } else { 1 };
                    for (col_idx, t) in row_texts.iter_mut().enumerate() {
                        let is_last_col = col_idx == num_cols.saturating_sub(1);
                        let base_w = widths.get(col_idx).copied().unwrap_or(0);
                        let target_w = if is_last_col {
                            base_w + surplus
                        } else {
                            base_w
                        } as usize;
                        let skip = if col_idx == target_prefix_col {
                            prefix_skip
                        } else {
                            0
                        };
                        apply_tier_underline(t, target_w, skip);
                    }
                }

                // push
                let mut row = Row::new(row_texts).height(height);

                if self.is_current(i) {
                    match self.config.row_connection {
                        RowConnectionStyle::Capped => {
                            row = row.style(self.config.inactive_current_style)
                        }
                        RowConnectionStyle::Full => row = row.style(self.config.current_style),
                        _ => {}
                    }
                } else if is_selected {
                    row = row.style(Style::from(self.config.selected_style));
                }

                rows.push(row);
            } else {
                let mut push = vec![];
                let mut vscroll_to_skip = if self.is_current(i) && self.config.vscroll_current_only
                {
                    self.vscroll as usize
                } else {
                    0
                };

                for (x, mut col) in row.into_iter().enumerate() {
                    if vscroll_to_skip > 0 {
                        let col_height = col.lines.len();
                        if vscroll_to_skip >= col_height {
                            vscroll_to_skip -= col_height;
                            continue;
                        } else {
                            col.lines = col.lines.split_off(vscroll_to_skip);
                            vscroll_to_skip = 0;
                        }
                    }

                    let mut height = col.height() as u16;

                    if remaining_height == 0 {
                        break;
                    } else if remaining_height < height {
                        height = remaining_height;
                        clip_text_lines(&mut col, remaining_height, self.reverse());
                    }
                    remaining_height -= height;

                    let is_current_row = self.is_current(i);
                    prefix_span(
                        &mut col,
                        prefix.clone(),
                        self.active_prefix_style(&icon_name_hz, is_selected, is_spinner, &cwd),
                        self.inactive_prefix_style(
                            &icon_name_hz,
                            is_selected && !is_current_row,
                            is_spinner,
                            &cwd,
                        ),
                        is_current_row,
                        if !is_selected && !is_yanked && !is_cut {
                            nav_bar_span.clone()
                        } else {
                            None
                        },
                        self.config.current_style,
                        self.config.current_nav_bar_style,
                    );
                    if self.config.icons && x == 0 {
                        insert_icon_span(
                            &mut col,
                            &icon_name_hz,
                            !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some(),
                            is_current_row,
                            self.config.uncolor_current_icon,
                            self.config.invert_current_icon,
                            self.config.current_icon_style,
                        );
                    }
                    if self.config.symlink_target && x == 0 {
                        maybe_append_symlink_target(
                            &mut col,
                            &icon_name_hz,
                            self.config.symlink_target_style.into(),
                            self.width,
                        );
                    }

                    let is_active_col = active_column == x;

                    match self.config.row_connection {
                        RowConnectionStyle::Disjoint => {
                            if is_active_col {
                                col = col.style(if is_current_row {
                                    self.config.current_style
                                } else {
                                    self.config.style
                                });
                            } else {
                                col = col.style(if is_current_row {
                                    self.config.inactive_current_style
                                } else {
                                    self.config.inactive_style
                                });
                            }
                        }
                        RowConnectionStyle::Capped => {
                            if is_active_col {
                                col = col.style(if is_current_row {
                                    self.config.current_style
                                } else {
                                    self.config.style
                                });
                            }
                        }
                        RowConnectionStyle::Full => {}
                    }

                    if is_underline_mode
                        && matches!(
                            drain_iter.peek(),
                            Some((Some(crate::nucleo::GroupHeader::TierSeparator), _, _))
                        )
                    {
                        let target_w = self.width as usize;
                        let has_nav =
                            !is_selected && !is_yanked && !is_cut && nav_bar_span.is_some();
                        let prefix_skip = if has_nav { 2 } else { 1 };
                        let skip = if x == 0 { prefix_skip } else { 0 };
                        apply_tier_underline(&mut col, target_w, skip);
                    }

                    // push
                    let mut row = Row::new(vec![col]).height(height);
                    if is_current_row {
                        match self.config.row_connection {
                            RowConnectionStyle::Capped => {
                                row = row.style(self.config.inactive_current_style)
                            }
                            RowConnectionStyle::Full => row = row.style(self.config.current_style),
                            _ => {}
                        }
                    } else if is_selected {
                        row = row.style(Style::from(self.config.selected_style));
                    }
                    push.push(row);
                }
                rows.extend(push);
            }
            i += 1;
        }

        // doesn't loop back after results is exhausted so we have to set here
        if let Click::ResultPos(_c) = click {
            log::debug!("Mapped click to last row = {i}");
            *click = Click::ResultIdx(self.bottom as u32 + i as u32 - 1);
        }

        if self.reverse() {
            rows.reverse();
            if remaining_height > 0 {
                rows.insert(0, Row::new(vec![vec![]]).height(remaining_height));
            }
        }

        // ratatui column_spacing eats into the constraints
        let table_widths = if as_cols {
            // first 0 element after which all is 0
            let pos = widths.iter().rposition(|&x| x != 0);
            // column_spacing eats into the width
            let mut widths: Vec<_> = widths[..pos.map_or(0, |x| x + 1)].to_vec();

            let total_available = self.width.saturating_sub(self.column_spacing_width());
            let surplus = total_available.saturating_sub(widths.iter().sum());

            if surplus > 0 {
                // occupy full row
                let is_single_col = widths.iter().filter(|&&w| w != 0).count() <= 1;
                if matches!(self.config.row_connection, RowConnectionStyle::Full)
                    || is_single_col
                    || (matches!(self.config.row_connection, RowConnectionStyle::Disjoint)
                        && self.config.right_align_last)
                {
                    if let Some(s) = widths.iter_mut().rfind(|w| **w != 0) {
                        *s += surplus;
                    }
                }
            }

            // save actual widths of each column
            self.widths = widths.clone();

            widths
        } else {
            vec![self.width]
        };

        // log::trace!(
        //     "limits: {width_limits:?}, widths: {widths:?}, {:?}, medians {:?}",
        //     self.width,
        //     self.medians
        // );

        let mut table = Table::new(rows, table_widths).column_spacing(self.config.column_spacing.0);

        table = match self.config.row_connection {
            RowConnectionStyle::Full => table.style(self.config.style),
            RowConnectionStyle::Capped => table.style(self.config.inactive_style),
            _ => table,
        };

        // log::trace!("{table:?}");

        if !self.config.border.is_empty() {
            table = table.block(self.config.border.as_static_block());
        }
        table
    }
}

impl ResultsUI {
    fn default_prefix(&self, i: usize) -> String {
        let substituted = substitute_escaped(
            &self.config.unselected_prefix,
            &[
                ('d', &(i + 1).to_string()),                        // cursor index
                ('r', &(i + 1 + self.bottom as usize).to_string()), // absolute index
            ],
        );

        fit_width(&substituted, self.config.multi_prefix.width())
    }

    fn is_current(&self, i: usize) -> bool {
        !self.cursor_disabled && self.cursor == i as u16
    }

    fn hr(&self) -> Option<Row<'static>> {
        let sep = self.config.separator;

        if matches!(sep, HorizontalSeparator::None) {
            return None;
        }

        let unit = sep.as_str();
        let line = unit.repeat(self.width as usize);

        // todo: support non_stacked properly by doing a seperate rendering pass
        if !self.config.stacked_columns && self.widths.len() > 1 {
            // Some(Row::new(vec![vec![]]))
            Some(Row::new(vec![line; self.widths().len()]).style(self.config.separator_style))
        } else {
            Some(Row::new(vec![line]).style(self.config.separator_style))
        }
    }

    fn _hr(&self) -> u16 {
        !matches!(self.config.separator, HorizontalSeparator::None) as u16
    }
}
