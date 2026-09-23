use unicode_width::UnicodeWidthStr;

use crate::{ui::results::ResultsUI, utils::string::allocate_widths};

impl ResultsUI {
    // ----- columns --------
    // todo: support cooler things like only showing/outputting a specific column/cycling columns
    pub fn toggle_col(&mut self, col_idx: usize) -> bool {
        self.reset_current_scroll();

        if self.col == Some(col_idx) {
            self.col = None
        } else {
            self.col = Some(col_idx);
        }
        self.col.is_some()
    }

    pub fn cycle_col(&mut self) {
        self.reset_current_scroll();

        self.col = match self.col {
            None => self.widths.is_empty().then_some(0),
            Some(c) => {
                let next = c + 1;
                if next < self.widths.len() {
                    Some(next)
                } else {
                    None
                }
            }
        };
    }

    // ------- LAYOUT & WIDTH CALCULATIONS ----------
    pub fn indentation(&self) -> usize {
        self.config.multi_prefix.width() + if self.config.icons { 3 } else { 0 }
    }

    pub fn col(&self) -> Option<usize> {
        self.col
    }

    /// Column widths.
    /// Note that the first width doesn't include the indentation.
    pub fn widths(&self) -> &Vec<u16> {
        &self.widths
    }

    /// Adapt the stored widths (initialized by [`Worker::results`]) to the fit within the available width (self.width)
    /// widths <= min_wrap_width don't shrink and aren't wrapped
    pub fn max_widths(&self) -> Vec<u16> {
        let mut base_widths = self.medians.clone();

        // uninitialized
        if base_widths.is_empty() || base_widths.iter().all(|x| *x == 0) {
            return vec![];
        }

        for w in base_widths.iter_mut() {
            *w = (*w).max(self.config.min_width);
        }

        base_widths.resize(self.hidden_columns.len().max(base_widths.len()), 0);

        for (i, is_hidden) in self.hidden_columns.iter().enumerate() {
            if *is_hidden {
                base_widths[i] = 0;
            }
        }

        let target = self.content_width();

        let sum: u16 = base_widths.iter().sum();

        if sum < target {
            let nonzero_count = base_widths.iter().filter(|w| **w > 0).count();

            let extra = target - sum;
            let extra_per_column = extra / nonzero_count as u16;
            let mut remainder = extra % nonzero_count as u16;

            for w in base_widths.iter_mut().filter(|w| **w > 0) {
                if *w > 0 {
                    *w += extra_per_column;

                    if remainder > 0 {
                        *w += 1;
                        remainder -= 1;
                    }
                }
            }
        }

        match allocate_widths(&base_widths, target, self.config.min_width) {
            Ok(s) | Err(s) => s,
        }
    }

    pub fn content_width(&self) -> u16 {
        self.width
            .saturating_sub(self.indentation() as u16)
            .saturating_sub(self.column_spacing_width())
    }

    pub fn column_spacing_width(&self) -> u16 {
        let pos = self.widths.iter().rposition(|&x| x != 0);
        self.config.column_spacing.0 * (pos.unwrap_or_default() as u16)
    }

    pub fn table_width(&self) -> u16 {
        if self.config.stacked_columns {
            self.width
        } else {
            self.widths.iter().sum::<u16>()
                + self.config.border.width()
                + self.column_spacing_width()
        }
    }
}
