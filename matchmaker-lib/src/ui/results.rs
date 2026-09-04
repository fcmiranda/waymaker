use std::collections::HashSet;

use ratatui::{layout::Rect, text::Line};

use crate::{
    config::{ResultsConfig, StatusConfig},
    nucleo::Status,
};

pub mod icons;
pub mod render;
pub mod status;
pub mod widths;

#[cfg(test)]
mod tests;

pub use icons::icon_for_name;
pub use status::StatusUI;

#[derive(Debug)]
pub struct ResultsUI {
    cursor: u16,
    bottom: u32,
    col: Option<usize>,
    pub hscroll: i8,
    pub vscroll: u8,

    /// available height
    height: u16,
    /// available width
    width: u16,
    // column widths.
    // Note that the first width include the indentation.
    widths: Vec<u16>,
    medians: Vec<u16>,

    pub hidden_columns: Vec<bool>,

    pub status: Status,
    status_template: Line<'static>,
    pub status_config: StatusConfig,

    pub config: ResultsConfig,
    pub initial_pos: Option<i32>,

    bottom_clip: Option<u16>,
    cursor_above: u16,

    pub cursor_disabled: bool,

    /// Set of col-0 names whose prefix should be rendered with `yank_prefix_style`.
    /// Populated externally via `Action::Custom(FmSetYankPaths(...))`.
    pub yank_paths: HashSet<String>,
    pub cut_paths: HashSet<String>,
    pub pin_paths: HashSet<String>,
    pub mode_index: usize,
}

impl ResultsUI {
    pub fn new(config: ResultsConfig, mut status_config: StatusConfig) -> Self {
        status_config.interactions.sort_by_key(|(i, _)| *i);

        Self {
            cursor: 0,
            bottom: 0,
            col: None,
            hscroll: 0,
            vscroll: 0,

            widths: Vec::new(),
            medians: Vec::new(),
            height: 0, // uninitialized, so be sure to call update_dimensions
            width: 0,
            hidden_columns: Default::default(),

            status: Default::default(),
            status_template: Line::from(status_config.template.clone()).style(status_config.style),
            status_config,
            initial_pos: config.pos,
            config,

            cursor_disabled: false,
            bottom_clip: None,
            cursor_above: 0,
            yank_paths: HashSet::new(),
            cut_paths: HashSet::new(),
            pin_paths: HashSet::new(),
            mode_index: 0,
        }
    }

    pub fn set_mode_index(&mut self, index: usize) {
        self.mode_index = index;
    }

    pub fn hidden_columns(&mut self, hidden_columns: Vec<bool>) {
        self.hidden_columns = hidden_columns;
    }

    // as given by ratatui area
    pub fn update_dimensions(&mut self, area: &Rect) {
        let [bw, bh] = [self.config.border.height(), self.config.border.width()];
        self.width = area.width.saturating_sub(bw);
        let old_height = self.height;
        self.height = area.height.saturating_sub(bh);
        if old_height == 0 && self.height > 0 {
            let abs_index = self.bottom + self.cursor as u32;
            let end = self.end();
            let index = abs_index.min(end);
            if index < self.bottom as u32 || index >= self.bottom + self.height as u32 {
                self.bottom = (end + 1).saturating_sub(self.height as u32).min(index);
            }
            self.cursor = (index - self.bottom) as u16;
        }
        log::debug!("Updated results dimensions: {}x{}", self.width, self.height);
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    // ------ config -------
    pub fn reverse(&self) -> bool {
        self.config.reverse == Some(true)
    }
    pub fn is_wrap(&self) -> bool {
        self.config.wrap
    }
    pub fn wrap(&mut self, wrap: bool) {
        self.config.wrap = wrap;
    }

    // ------- NAVIGATION ---------
    fn scroll_padding(&self) -> u16 {
        self.config.scroll_padding.min(self.height / 2)
    }
    pub fn end(&self) -> u32 {
        self.status.matched_count.saturating_sub(1)
    }

    /// Index in worker snapshot of current item.
    /// Use with worker.get_nth().
    //  Equivalently, the cursor progress in the match list
    pub fn index(&self) -> u32 {
        if self.cursor_disabled {
            u32::MAX
        } else {
            self.cursor as u32 + self.bottom
        }
    }

    pub fn cursor_offset(&self) -> Option<u16> {
        if self.cursor_disabled {
            None
        } else {
            Some(self.cursor_above)
        }
    }

    /// Returns whether scroll wrap caused it to jump to the end
    pub fn cursor_prev(&mut self) -> bool {
        self.reset_current_scroll();

        if (self.cursor_above <= self.scroll_padding() || self.cursor <= self.scroll_padding())
            && self.bottom > 0
        {
            self.bottom -= 1;
            self.bottom_clip = None;
        } else if self.cursor > 0 {
            self.cursor -= 1;
        } else if self.config.scroll_wrap {
            log::trace!("d");

            log::trace!(
                "Cursor prev caused jump: above: {} bottom: {}",
                self.cursor_above,
                self.bottom
            );
            self.cursor_jump(self.end());
            return true;
        }

        false
    }

    /// Returns whether scroll wrap caused it to jump to start
    pub fn cursor_next(&mut self) -> bool {
        self.reset_current_scroll();

        if self.cursor_disabled {
            self.cursor_disabled = false
        }

        if self.cursor + 1 + self.scroll_padding() >= self.height
            && self.bottom + (self.height as u32) < self.status.matched_count
        {
            self.bottom += 1;
        } else if self.index() < self.end() {
            self.cursor += 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(0);
            return true;
        }
        false
    }

    pub fn cursor_jump(&mut self, index: u32) {
        self.reset_current_scroll();

        self.cursor_disabled = false;
        self.bottom_clip = None;

        let end = self.end();
        let index = index.min(end);

        if self.height == 0 {
            self.bottom = 0;
            self.cursor = index as u16;
        } else {
            if index < self.bottom as u32 || index >= self.bottom + self.height as u32 {
                self.bottom = (end + 1)
                    .saturating_sub(self.height as u32) // don't exceed the first item of the last self.height items
                    .min(index);
            }
            self.cursor = (index - self.bottom) as u16;
        }
        log::debug!("cursor jumped to {}: {index}, end: {end}", self.cursor);
    }

    pub fn current_scroll(&mut self, x: i8, horizontal: bool) {
        if horizontal {
            self.hscroll = if x == 0 {
                0
            } else {
                self.hscroll.saturating_add(x)
            };
        } else {
            self.vscroll = if x == 0 {
                0
            } else if x.is_negative() {
                self.vscroll.saturating_add(x.unsigned_abs())
            } else {
                self.vscroll.saturating_sub(x as u8)
            };
        }
    }

    pub fn reset_current_scroll(&mut self) {
        self.hscroll = 0;
        self.vscroll = 0;
    }
}
