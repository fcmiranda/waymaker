import sys
import re

with open("waymaker-lib/src/ui/results.rs", "r") as f:
    content = f.read()

# 1. Replace nav_bar_span and dynamic_multi_prefix with closures
old_block = """        let nav_bar_span = nav_bar_style.map(|(border_type, style)| {
            let border_char = match border_type {
                ratatui::widgets::BorderType::Plain => "╷".to_string(),
                ratatui::widgets::BorderType::Rounded => "╵".to_string(),
                ratatui::widgets::BorderType::Double => "║".to_string(),
                ratatui::widgets::BorderType::Thick => "▌".to_string(),
                ratatui::widgets::BorderType::QuadrantInside => "▐".to_string(),
                ratatui::widgets::BorderType::QuadrantOutside => "▌".to_string(),
                _ => "╷".to_string(),
            };
            ratatui::text::Span::styled(border_char, style)
        });

        let dynamic_multi_prefix = if let Some((border_type, _)) = nav_bar_style {
            let border_char = match border_type {
                ratatui::widgets::BorderType::Plain => "╷",
                ratatui::widgets::BorderType::Rounded => "╵",
                ratatui::widgets::BorderType::Double => "║",
                ratatui::widgets::BorderType::Thick => "▌",
                ratatui::widgets::BorderType::QuadrantInside => "▐",
                ratatui::widgets::BorderType::QuadrantOutside => "▌",
                _ => "╷",
            };
            if let Some(_first_char) = self.config.multi_prefix.chars().next() {
                let rest: String = self.config.multi_prefix.chars().skip(1).collect();
                format!("{}{}", border_char, rest)
            } else {
                format!("{} ", border_char)
            }
        } else {
            self.config.multi_prefix.clone()
        };"""

new_block = """        let get_border_char = |is_first: bool, is_last: bool, border_type: ratatui::widgets::BorderType| -> &'static str {
            match border_type {
                ratatui::widgets::BorderType::Plain | ratatui::widgets::BorderType::Rounded => {
                    if is_first && is_last {
                        "│"
                    } else if is_first {
                        "╷"
                    } else if is_last {
                        "╵"
                    } else {
                        "│"
                    }
                },
                ratatui::widgets::BorderType::Double => "║",
                ratatui::widgets::BorderType::Thick => "▌",
                ratatui::widgets::BorderType::QuadrantInside => "▐",
                ratatui::widgets::BorderType::QuadrantOutside => "▌",
                _ => "│",
            }
        };

        let get_nav_bar_span = |is_first: bool, is_last: bool| -> Option<ratatui::text::Span<'a>> {
            nav_bar_style.clone().map(|(border_type, style)| {
                let border_char = get_border_char(is_first, is_last, border_type);
                ratatui::text::Span::styled(border_char.to_string(), style)
            })
        };

        let get_dynamic_multi_prefix = |is_first: bool, is_last: bool| -> String {
            if let Some((border_type, _)) = nav_bar_style.clone() {
                let border_char = get_border_char(is_first, is_last, border_type);
                if let Some(_first_char) = self.config.multi_prefix.chars().next() {
                    let rest: String = self.config.multi_prefix.chars().skip(1).collect();
                    format!("{}{}", border_char, rest)
                } else {
                    format!("{} ", border_char)
                }
            } else {
                self.config.multi_prefix.clone()
            }
        };"""

content = content.replace(old_block, new_block)

# 2. Modify get_prefix! definition
old_get_prefix = """        macro_rules! get_prefix {
            ($row:expr, $is_selected:expr, $idx:expr, $item:expr, $columns:expr) => {{"""
new_get_prefix = """        macro_rules! get_prefix {
            ($row:expr, $is_selected:expr, $idx:expr, $item:expr, $columns:expr, $is_first:expr, $is_last:expr) => {{"""
content = content.replace(old_get_prefix, new_get_prefix)

old_dyn_prefix = """                } else if $is_selected || is_yanked || is_cut {
                    dynamic_multi_prefix.clone()
                } else {"""
new_dyn_prefix = """                } else if $is_selected || is_yanked || is_cut {
                    get_dynamic_multi_prefix($is_first, $is_last)
                } else {"""
content = content.replace(old_dyn_prefix, new_dyn_prefix)

# 3. Call site 1
old_call_1 = """                    let (prefix, icon_name, is_spinner, spinner_col_idx, is_yanked, is_cut) = get_prefix!(row, is_selected, 0, item, columns);"""
new_call_1 = """                    let is_first = rows.is_empty();
                    let is_last = (self.height <= total_height + remaining_height) || (start_index >= results.len());
                    let nav_bar_span = get_nav_bar_span(is_first, is_last);
                    let (prefix, icon_name, is_spinner, spinner_col_idx, is_yanked, is_cut) = get_prefix!(row, is_selected, 0, item, columns, is_first, is_last);"""
content = content.replace(old_call_1, new_call_1)

# 4. Call site 2
old_call_2 = """            let (prefix, icon_name, is_spinner, spinner_col_idx, is_yanked, is_cut) = get_prefix!(row, is_selected, 0, item, columns);"""
new_call_2 = """            let is_first = rows.is_empty();
            let is_last = (self.height <= total_height + remaining_height) || (start_index >= results.len());
            let nav_bar_span = get_nav_bar_span(is_first, is_last);
            let (prefix, icon_name, is_spinner, spinner_col_idx, is_yanked, is_cut) = get_prefix!(row, is_selected, 0, item, columns, is_first, is_last);"""
content = content.replace(old_call_2, new_call_2)

# 5. Call site 3
old_call_3_loop = """        for (group, mut row, item) in results.drain(start_index as usize..) {"""
new_call_3_loop = """        let mut drain_iter = results.drain(start_index as usize..).peekable();
        while let Some((group, mut row, item)) = drain_iter.next() {"""
content = content.replace(old_call_3_loop, new_call_3_loop)

old_call_3 = """            let (prefix, icon_name_hz, is_spinner, spinner_col_idx, is_yanked, is_cut) = get_prefix!(row, is_selected, i, item, columns);"""
new_call_3 = """            let is_first = rows.is_empty();
            let is_last_in_results = drain_iter.peek().is_none();
            let h = if as_cols {
                row.iter().map(|t| t.height() as u16).max().unwrap_or_default()
            } else {
                row.iter().map(|t| t.height() as u16).sum::<u16>()
            };
            let is_last = is_last_in_results || (remaining_height <= h);
            let nav_bar_span = get_nav_bar_span(is_first, is_last);
            let (prefix, icon_name_hz, is_spinner, spinner_col_idx, is_yanked, is_cut) = get_prefix!(row, is_selected, i, item, columns, is_first, is_last);"""
content = content.replace(old_call_3, new_call_3)

# Write back
with open("waymaker-lib/src/ui/results.rs", "w") as f:
    f.write(content)

print("Done")
