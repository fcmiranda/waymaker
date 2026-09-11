use log::error;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::{
    config::{
        BorderSetting, PreviewConfig, PreviewInitialSetting, PreviewSetting, ShowCondition, Side,
    },
    preview::Preview,
    utils::text::wrapped_line_height,
};

pub struct PreviewUI {
    pub view: Preview,
    pub config: PreviewConfig,
    layout_idx: usize,
    /// content area
    pub(crate) area: Rect,
    pub scroll: [u16; 2],
    offset: usize,
    target: Option<usize>,
    attained_target: bool,
    #[cfg(feature = "partial")]
    initial: PreviewInitialSetting,

    pub last_count: usize,

    pub jump: (bool, usize), // end, initial

    show: bool,

    pub current_dimension: Option<u16>,

    /// Dynamic title set from the current item's first column; shown in the
    /// preview border when the border is configured.
    title: Option<String>,

    picker: Option<ratatui_image::picker::Picker>,
    pub zoom: f32,
    pub show_diagram: bool,
    pub image_state: Option<ratatui_image::protocol::StatefulProtocol>,
    current_image_id: u64,
    last_image_area: Rect,
}

impl PreviewUI {
    fn active_border(&self) -> Option<&BorderSetting> {
        if self.is_fullscreen() {
            return None;
        }
        if let Some(layout_border) = self.setting().and_then(|s| s.border.as_ref()) {
            if !layout_border.is_empty() {
                return Some(layout_border);
            }
        }

        (!self.config.border.is_empty()).then_some(&self.config.border)
    }

    fn initial(&self) -> &PreviewInitialSetting {
        #[cfg(feature = "partial")]
        {
            &self.initial
        }
        #[cfg(not(feature = "partial"))]
        {
            &self.config.initial
        }
    }

    pub fn new(view: Preview, mut config: PreviewConfig, [ui_width, ui_height]: [u16; 2]) -> Self {
        for x in &mut config.layout {
            if let Some(b) = &mut x.border
                && b.sides.is_none()
                && !b.is_empty()
            {
                b.sides = Some(x.layout.side.opposite())
            }
        }

        let show = match config.show {
            ShowCondition::Free(x) => {
                if let Some(l) = config.layout.first() {
                    match l.layout.side {
                        Side::Bottom | Side::Top => ui_height >= x,
                        _ => ui_width >= x,
                    }
                } else {
                    false
                }
            }
            ShowCondition::Bool(x) => {
                x && if let Some(l) = config.layout.first() {
                    (match l.layout.side {
                        Side::Bottom | Side::Top => ui_height,
                        _ => ui_width,
                    }) > 5 + (l.layout.min.max(0) as u16)
                } else {
                    false
                }
            }
        };

        if config.layout.iter().any(|l| l.layout.max > 0) {
            config.layout.retain(|l| l.layout.max > 0);
        }

        // enforce invariant of valid index
        if config.layout.is_empty() {
            let mut s = PreviewSetting::default();
            s.layout.max = 0;
            config.layout.push(s);
        }

        let mut picker = None;
        if config.media {
            if std::env::var("TMUX").is_ok()
                && std::env::var("TERM_PROGRAM").map(|v| v != "tmux").unwrap_or(true)
            {
                #[allow(unused_unsafe)]
                unsafe {
                    std::env::set_var("TERM_PROGRAM", "tmux");
                }
            }

            use std::io::IsTerminal;
            let mut p = if std::io::stdout().is_terminal() {
                ratatui_image::picker::Picker::from_query_stdio()
                    .or_else(|_| query_tty_picker(std::time::Duration::from_millis(100)))
                    .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
            } else {
                query_tty_picker(std::time::Duration::from_millis(100))
                    .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks())
            };

            if let Some(ref protocol_str) = config.media_protocol {
                let protocol_type = match protocol_str.to_ascii_lowercase().as_str() {
                    "kitty" => Some(ratatui_image::picker::ProtocolType::Kitty),
                    "sixel" => Some(ratatui_image::picker::ProtocolType::Sixel),
                    "halfblocks" => Some(ratatui_image::picker::ProtocolType::Halfblocks),
                    "iterm2" => Some(ratatui_image::picker::ProtocolType::Iterm2),
                    _ => None,
                };
                if let Some(pt) = protocol_type {
                    p.set_protocol_type(pt);
                }
            } else if p.protocol_type() == ratatui_image::picker::ProtocolType::Halfblocks {
                // If stdio query timed out or failed (e.g. inside tmux or subshell),
                // ensure media previews always render with high-resolution Kitty protocol.
                p.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
            }
            picker = Some(p);
            if std::env::var("TMUX").is_ok() {
                let _ = std::process::Command::new("tmux")
                    .args(["set", "-p", "allow-passthrough", "all"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }

        let zoom = config.zoom.unwrap_or(1.0);

        Self {
            view,
            #[cfg(feature = "partial")]
            initial: config.initial.clone(),
            config,
            area: Rect::default(),
            layout_idx: 0,
            scroll: [0, 0],
            offset: 0,
            target: None,
            attained_target: false,
            last_count: 0,
            jump: (false, 0),
            show,
            current_dimension: None,
            title: None,
            picker,
            zoom,
            show_diagram: false,
            image_state: None,
            current_image_id: 0,
            last_image_area: Rect::default(),
        }
    }

    pub fn update_dimensions(&mut self, area: &Rect) {
        let (border_h, border_w) = self
            .active_border()
            .map(|b| (b.height(), b.width()))
            .unwrap_or((0, 0));
        let mut height = area.height;
        height -= border_h.min(height);
        self.area.height = height;

        let mut width = area.width;
        width -= border_w.min(width);
        self.area.width = width;
    }

    pub fn reevaluate_show_condition(&mut self, [ui_width, ui_height]: [u16; 2], hide: bool) {
        match self.config.show {
            ShowCondition::Free(x) => {
                if let Some(setting) = self.setting() {
                    let l = &setting.layout;

                    let show = match l.side {
                        Side::Bottom | Side::Top => ui_height >= x,
                        _ => ui_width >= x,
                    };
                    log::debug!(
                        "Evaluated ShowCondition(Free({x})) against {ui_width}x{ui_height} => {show}"
                    );
                    if !hide && !show {
                        return;
                    }

                    self.show(show);
                };
            }
            ShowCondition::Bool(mut show) => {
                if !hide && !show {
                    return;
                };
                show = show
                    && if let Some(l) = self.config.layout.first() {
                        (match l.layout.side {
                            Side::Bottom | Side::Top => ui_height,
                            _ => ui_width,
                        }) > 5 + (l.layout.min.max(0) as u16)
                    } else {
                        false
                    };
                self.show(show);
            }
        };
    }

    // -------- Setting getters -----------
    /// Set the dynamic item title shown in the preview border.
    pub fn set_title(&mut self, title: Option<String>) {
        if self.title != title {
            self.show_diagram = false;
            self.zoom = 1.0;
        }
        self.title = title;
    }

    /// None if not show OR if max = 0 (disabled layour)
    pub fn setting(&self) -> Option<&PreviewSetting> {
        // if let Some(ret) = self.config.layout.get(self.layout_idx)
        if let ret = &self.config.layout[self.layout_idx]
            && ret.layout.max != 0
        {
            Some(&ret)
        } else {
            None
        }
    }

    pub fn setting_mut(&mut self) -> Option<&mut PreviewSetting> {
        if let Some(ret) = self.config.layout.get_mut(self.layout_idx)
            && ret.layout.max != 0
        {
            Some(ret)
        } else {
            None
        }
    }

    pub fn visible(&self) -> bool {
        self.setting().is_some() && self.show
    }

    pub fn command(&self) -> &str {
        self.setting().map(|x| x.command.as_str()).unwrap_or("")
    }

    pub fn border(&self) -> &BorderSetting {
        self.setting()
            .and_then(|s| s.border.as_ref())
            .unwrap_or(&self.config.border)
    }

    pub fn get_initial_command(&self) -> &str {
        let x = self.command();
        if !x.is_empty() {
            return x;
        }

        self.config
            .layout
            .iter()
            .map(|l| l.command.as_str())
            .find(|cmd| !cmd.is_empty())
            .unwrap_or("")
    }

    // -------- Layout -----------
    pub fn cycle_layout(&mut self) {
        let len = self.config.layout.len();

        for _ in 0..len {
            self.layout_idx = (self.layout_idx + 1) % len;

            if self.config.layout[self.layout_idx].layout.max > 0 {
                self.reinit();
                return;
            }
        }
    }
    pub fn set_layout(&mut self, idx: u8) -> bool {
        let idx = idx as usize;
        if idx < self.config.layout.len() {
            let changed = self.layout_idx != idx;
            self.layout_idx = idx;
            self.reinit();
            changed
        } else {
            error!("Layout idx {idx} out of bounds, ignoring.");
            false
        }
    }
    pub fn reinit(&mut self) {
        #[cfg(feature = "partial")]
        {
            use matchmaker_partial::Apply;
            if let Some(s) = self.setting() {
                let mut new = self.config.initial.clone();
                new.apply(s.initial.clone());
                log::trace!("Applied: {:?} -> {:?}", s.initial, new);
                self.initial = new;
            }
        }
        self.current_dimension = None;
    }

    pub fn is_fullscreen(&self) -> bool {
        self.setting()
            .map(|s| s.layout.percentage.inner() >= 90)
            .unwrap_or(false)
    }

    pub fn toggle_diagram(&mut self) {
        self.show_diagram = !self.show_diagram;
        self.view
            .image_id
            .fetch_add(1, std::sync::atomic::Ordering::Release);
        self.view
            .changed
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn has_markdown(&self) -> bool {
        self.view.has_string()
    }

    pub fn has_diagram(&self) -> bool {
        self.view
            .diagram_sources
            .lock()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    // ----- config && getters ---------

    pub fn show(&mut self, show: bool) -> bool {
        log::trace!("toggle preview with: {show}");
        let changed = self.show != show;
        self.show = show;
        changed
    }

    pub fn toggle_show(&mut self) {
        self.show = !self.show;
    }

    pub fn wrap(&mut self, wrap: bool) {
        self.config.wrap = wrap;
    }
    pub fn is_wrap(&self) -> bool {
        self.config.wrap
    }
    pub fn offset(&self) -> usize {
        self.initial().header_lines + self.offset
    }
    pub fn target_line(&self) -> Option<usize> {
        self.target
    }

    // ----- actions --------
    pub fn up(&mut self, n: u16) {
        let total_lines = self.view.len();
        let n = n as usize;

        if self.offset >= n {
            self.offset -= n;
        } else if self.config.scroll_wrap {
            self.offset = total_lines.saturating_sub(n - self.offset);
        } else {
            self.offset = 0;
        }
    }
    pub fn down(&mut self, n: u16) {
        let total_lines = self.view.len();
        let n = n as usize;

        if self.offset + n > total_lines {
            if self.config.scroll_wrap {
                self.offset = 0;
            } else {
                self.offset = total_lines;
            }
        } else {
            self.offset += n;
        }
    }

    /// Jump directly to a specific line in the preview.
    pub fn scroll_to(&mut self, line: usize) {
        let total_lines = self.view.len();
        self.offset = line.min(total_lines.saturating_sub(1));
    }

    /// Current vertical line offset in the preview.
    pub fn current_offset(&self) -> usize {
        self.offset
    }

    pub fn scroll(&mut self, horizontal: bool, val: i8) {
        let a = &mut self.scroll[horizontal as usize];

        if val == 0 {
            *a = 0;
        } else {
            let new = (*a as i8 + val).clamp(0, u16::MAX as i8);
            *a = new as u16;
        }
    }

    pub fn set_target(&mut self, target: Option<isize>) {
        if self.initial().tail {
            return;
        }

        let line_count = self.view.len();

        let Some(mut target) = target else {
            self.target = None;
            self.offset = 0;
            return;
        };

        target += self.initial().offset;

        self.target = Some(if target < 0 {
            line_count.saturating_sub(target.unsigned_abs())
        } else {
            target as usize
        });

        let index = self.target.unwrap();

        self.offset = if index >= line_count {
            self.attained_target = false;
            line_count.saturating_sub(self.area.height as usize / 2)
        } else {
            self.attained_target = true;
            self.target_to_offset(index)
        };

        log::trace!("Preview initial offset: {}, index: {}", self.offset, index);
    }

    pub fn jump(&mut self) {
        if self.initial().tail {
            if self.offset > 0 {
                // go to end
                self.jump = (false, self.offset);
                self.reset_scroll();
            } else {
                if !self.jump.0 {
                    // go to start

                    self.attained_target = true;
                    self.offset = 0;
                    self.jump.0 = true
                } else {
                    // go to saved
                    self.offset = self.jump.1;
                    self.attained_target = true;
                    self.jump = (false, 0)
                }
            }
        } else {
            match self.jump {
                (false, 0) => {
                    self.jump = (true, self.offset);
                    self.scroll_end();
                }
                (true, x) if x != 0 => {
                    self.jump.0 = false;
                    self.reset_scroll();
                }
                _ => {
                    self.offset = self.jump.1;
                    self.jump = (false, 0)
                }
            }
        }
    }
    pub fn reset_scroll(&mut self) {
        self.offset = 0;
        self.attained_target = false;
    }
    pub fn scroll_end(&mut self) {
        let rl = self.view.len();
        let height = self.area.height as usize;

        let header_count = self.initial().header_lines.min(height);
        let remaining_lines = rl.saturating_sub(header_count);

        self.offset = remaining_lines.saturating_sub(height);
    }

    fn target_to_offset(&self, mut target: usize) -> usize {
        // decrement the index to put the target lower on the page.
        // The resulting height up to the top of target should >= p% of height.
        let mut lines_above =
            self.config
                .initial
                .percentage
                .complement()
                .compute_clamped(self.area.height, 0, 0);

        // shoddy approximation to how Paragraph wraps lines
        while target > 0 && lines_above > 0 {
            let prev = self
                .view
                .get_line(target)
                .map(|x| wrapped_line_height(&x, self.area.width))
                .unwrap_or(1);
            if prev > lines_above {
                break;
            } else {
                target -= 1;
                lines_above -= prev;
            }
        }

        target
    }
    // --------------------------

    pub fn drag_width(&self) -> u16 {
        if self.is_fullscreen() {
            return 0;
        }
        self.config.drag_width.unwrap_or_else(|| {
            let side = self
                .setting()
                .map(|s| &s.layout.side)
                .unwrap_or(&Side::Right);
            match side {
                Side::Left | Side::Right => self.active_border().map(|b| b.width()).unwrap_or(0),
                Side::Top | Side::Bottom => self.active_border().map(|b| b.height()).unwrap_or(0),
            }
        })
    }

    pub fn split(&self, area: Rect) -> [Rect; 3] {
        let Some(setting) = self.setting() else {
            return [Rect::default(), area, Rect::default()];
        };

        setting.layout.split(area, self.current_dimension)
    }

    pub fn expand(&mut self, n: u16) {
        if n == 0 {
            self.current_dimension = None;
            return;
        }
        let current = self.current_size();
        self.current_dimension = Some(current.saturating_add(n));
    }

    pub fn shrink(&mut self, n: u16) {
        if n == 0 {
            self.current_dimension = None;
            return;
        }

        let current = self.current_size();
        self.current_dimension = Some(current.saturating_sub(n));
    }

    fn current_size(&self) -> u16 {
        if let Some(dim) = self.current_dimension {
            dim
        } else {
            let setting = self.setting();
            let side = setting.map(|s| &s.layout.side).unwrap_or(&Side::Right);
            match side {
                Side::Left | Side::Right => {
                    self.area.width + self.active_border().map(|b| b.width()).unwrap_or(0)
                }
                Side::Top | Side::Bottom => {
                    self.area.height + self.active_border().map(|b| b.height()).unwrap_or(0)
                }
            }
        }
    }

    pub fn get_image_state(&mut self) -> Option<&mut ratatui_image::protocol::StatefulProtocol> {
        let live_image_id = self
            .view
            .image_id
            .load(std::sync::atomic::Ordering::Acquire);

        let area_changed = self.area != self.last_image_area;

        if live_image_id != self.current_image_id || area_changed {
            self.current_image_id = live_image_id;
            self.last_image_area = self.area;
            self.image_state = None;

            let image_opt = if let Ok(guard) = self.view.image.lock() {
                guard.clone()
            } else {
                None
            };

            if let Some(img) = image_opt
                && let Some(picker) = self.picker.as_ref()
            {
                let (font_w, font_h) = {
                    let sz = picker.font_size();
                    (sz.width.max(1) as f32, sz.height.max(1) as f32)
                };

                let avail_px_w = (self.area.width as f32 * font_w).max(10.0);
                let avail_px_h = (self.area.height as f32 * font_h).max(10.0);

                let img_w = img.width() as f32;
                let img_h = img.height() as f32;

                let (base_w, base_h) =
                    if img_w > 0.0 && img_h > 0.0 && self.area.width > 0 && self.area.height > 0 {
                        let fit_scale = (avail_px_w / img_w).min(avail_px_h / img_h);
                        let bw = ((img_w * fit_scale).round() as u32).max(4);
                        let bh = ((img_h * fit_scale).round() as u32).max(4);
                        (bw, bh)
                    } else {
                        (img.width().max(4), img.height().max(4))
                    };

                let zoom = self.zoom;
                let display_img = if zoom > 1.001 {
                    let crop_w = ((img.width() as f32 / zoom) as u32).clamp(1, img.width());
                    let crop_h = ((img.height() as f32 / zoom) as u32).clamp(1, img.height());
                    let cropped = img.crop_imm(0, 0, crop_w, crop_h);
                    cropped.resize_exact(
                        base_w,
                        base_h,
                        image::imageops::FilterType::Triangle,
                    )
                } else if zoom < 0.999 && zoom > 0.05 {
                    let scaled_w = ((base_w as f32 * zoom) as u32).clamp(1, base_w);
                    let scaled_h = ((base_h as f32 * zoom) as u32).clamp(1, base_h);
                    let scaled = img.resize_exact(
                        scaled_w,
                        scaled_h,
                        image::imageops::FilterType::Triangle,
                    );
                    let mut canvas = image::RgbaImage::from_pixel(
                        base_w,
                        base_h,
                        image::Rgba([0, 0, 0, 0]),
                    );
                    image::imageops::overlay(&mut canvas, &scaled.to_rgba8(), 0, 0);
                    image::DynamicImage::ImageRgba8(canvas)
                } else {
                    img.resize_exact(base_w, base_h, image::imageops::FilterType::Triangle)
                };
                let state = picker.new_resize_protocol(display_img);
                self.image_state = Some(state);
            }
        }

        self.image_state.as_mut()
    }

    /// Compute a centered sub-rectangle within `inner_area` matching the image's cell aspect ratio
    pub fn get_centered_image_area(&self, inner_area: Rect) -> Rect {
        if inner_area.width == 0 || inner_area.height == 0 {
            return inner_area;
        }

        let img_dims = if let Ok(guard) = self.view.image.lock() {
            guard.as_ref().map(|img| (img.width(), img.height()))
        } else {
            None
        };

        let Some((img_w, img_h)) = img_dims else {
            return inner_area;
        };

        if img_w == 0 || img_h == 0 {
            return inner_area;
        }

        let (font_w, font_h) = if let Some(p) = self.picker.as_ref() {
            let sz = p.font_size();
            (sz.width.max(1) as f32, sz.height.max(1) as f32)
        } else {
            (10.0, 20.0)
        };

        let cell_w = img_w as f32 / font_w;
        let cell_h = img_h as f32 / font_h;

        let scale = (inner_area.width as f32 / cell_w).min(inner_area.height as f32 / cell_h);
        let target_w = (cell_w * scale).round() as u16;
        let target_h = (cell_h * scale).round() as u16;

        let target_w = target_w.clamp(1, inner_area.width);
        let target_h = target_h.clamp(1, inner_area.height);

        let offset_x = (inner_area.width - target_w) / 2;
        let offset_y = (inner_area.height - target_h) / 2;

        Rect {
            x: inner_area.x + offset_x,
            y: inner_area.y + offset_y,
            width: target_w,
            height: target_h,
        }
    }

    fn title_text(&self) -> Option<String> {
        let configured_title = self.setting().and_then(|s| s.title.as_deref());
        let dynamic = self.title.as_deref().unwrap_or_default();
        let mut base_title = match configured_title {
            None => dynamic.to_string(),
            Some("") => String::new(),
            Some("{item}") => dynamic.to_string(),
            Some(t) if t.contains("{item}") => t.replace("{item}", dynamic),
            Some("$currentItemName") => dynamic.to_string(),
            Some(t) if t.contains("$currentItemName") => {
                t.replace("$currentItemName", dynamic)
            }
            Some(t) => t.to_string(),
        };

        if self.show_diagram {
            if let Ok(sources) = self.view.diagram_sources.lock() {
                let total = sources.len();
                if total > 0 {
                    let cur = self
                        .view
                        .current_diagram_idx
                        .load(std::sync::atomic::Ordering::Relaxed)
                        + 1;
                    let pct = (self.zoom * 100.0).round() as u32;
                    let diag_badge = if pct != 100 {
                        format!(" [Diagram {cur}/{total} · {pct}%]")
                    } else {
                        format!(" [Diagram {cur}/{total}]")
                    };
                    if base_title.is_empty() {
                        base_title = diag_badge.trim().to_string();
                    } else {
                        base_title.push_str(&diag_badge);
                    }
                }
            }
        }

        if base_title.is_empty() {
            None
        } else {
            Some(base_title)
        }
    }

    pub fn make_block<'a>(&'a self) -> Option<ratatui::widgets::Block<'a>> {
        if let Some(border) = self.active_border() {
            let mut block = border.as_block();
            if let Some(title) = self.title_text() {
                let fg = if border.title_fg != ratatui::style::Color::Reset {
                    border.title_fg
                } else if self.config.border.title_fg != ratatui::style::Color::Reset {
                    self.config.border.title_fg
                } else if border.color != ratatui::style::Color::Reset {
                    border.color
                } else {
                    self.config.border.color
                };
                block = block.title(ratatui::text::Span::styled(
                    title,
                    ratatui::style::Style::default()
                        .fg(fg)
                        .add_modifier(border.title_modifier),
                ));
            }
            Some(block)
        } else {
            None
        }
    }

    pub fn make_preview(&mut self) -> Paragraph<'_> {
        let rl = self.view.len();
        let height = self.area.height as usize;
        let mut offset = self.offset;

        // this only triggers on preview change but not guaranteed on every preview change -- attaching it to the event handler is worse
        if rl < self.last_count {
            self.offset = 0;
            self.attained_target = false;
            self.jump = (false, 0)
        }
        self.last_count = rl;

        if self.initial().tail && !self.attained_target {
            let header_count = self.initial().header_lines.min(height);
            let remaining_lines = rl.saturating_sub(header_count);
            let remaining_space = height.saturating_sub(header_count);

            // get current offset
            offset = remaining_lines.saturating_sub(remaining_space);
            // apply initial offset
            if self.initial().offset < 0 {
                offset = offset.saturating_sub((self.initial().offset).unsigned_abs());
            }

            // stop scrolling
            if self.offset != 0 {
                if self.offset > offset || self.offset + offset > rl {
                    self.offset = self.offset.saturating_sub(rl.saturating_sub(offset));
                } else {
                    self.offset += offset;
                }
                self.attained_target = true;
            }
            // log::trace!("{} {} {}", offset, self.offset, self.attained_target);
        } else if let Some(target) = self.target
            && !self.attained_target
            && target < rl
        {
            self.offset = self.target_to_offset(target);
            self.attained_target = true;
        };

        if height == 0 {
            return Paragraph::new(Vec::new());
        }

        let mut lines = Vec::with_capacity(height);

        let header_count = self.initial().header_lines.min(height);
        if header_count > 0 {
            lines.extend(self.view.results_window(0, header_count));
        }

        let remaining_height = height.saturating_sub(header_count);
        if remaining_height > 0 {
            lines.extend(
                self.view
                    .results_window(self.initial().header_lines + offset, remaining_height),
            );
        }

        let title_text = self.title_text();

        if self.active_border().is_none() {
            if let Some(title) = &title_text {
                let is_media = self.config.media && {
                    let p = std::path::Path::new(title);
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "png"
                                | "jpg"
                                | "jpeg"
                                | "gif"
                                | "webp"
                                | "bmp"
                                | "ico"
                                | "tiff"
                                | "pdf"
                                | "mp4"
                                | "mkv"
                                | "webm"
                                | "mov"
                                | "avi"
                                | "flv"
                                | "m4v"
                                | "wmv"
                        )
                    } else {
                        false
                    }
                };

                if !is_media {
                    let fg = if self.config.border.title_fg == ratatui::style::Color::Reset {
                        self.config.border.color
                    } else {
                        self.config.border.title_fg
                    };
                    let title_line = Line::from(Span::styled(
                        title.clone(),
                        Style::default()
                            .fg(fg)
                            .add_modifier(self.config.border.title_modifier),
                    ));
                    lines.insert(0, title_line);
                    lines.truncate(height);
                }
            }
        }

        let mut preview = Paragraph::new(lines);
        if let Some(block) = self.make_block() {
            preview = preview.block(block);
        }
        if self.config.wrap {
            preview = preview
                .wrap(Wrap { trim: false })
                .scroll(self.scroll.into());
        }
        preview
    }

    /// Draw the optional preview scrollbar on the right border of `area`.
    pub fn render_scrollbar(&self, frame: &mut Frame, area: Rect) {
        if !self.config.scrollbar || area.width == 0 || area.height < 2 {
            return;
        }

        let total_lines = self.view.len();
        let visible_height = self.area.height as usize;
        let offset = self.offset;

        let (top_y, bottom_y, style) = if let Some(border) = self.active_border() {
            let sides = border.sides();
            let has_top = sides.intersects(ratatui::widgets::Borders::TOP);
            let has_bottom = sides.intersects(ratatui::widgets::Borders::BOTTOM);
            let top_y = if has_top { area.y + 1 } else { area.y };
            let bottom_y = if has_bottom {
                area.bottom().saturating_sub(1)
            } else {
                area.bottom()
            };
            let border_color = if border.color != ratatui::style::Color::Reset {
                border.color
            } else if self.config.border.color != ratatui::style::Color::Reset {
                self.config.border.color
            } else {
                ratatui::style::Color::DarkGray
            };
            let style = Style::default()
                .fg(border_color)
                .bg(border.bg)
                .add_modifier(border.modifier);
            (top_y, bottom_y, style)
        } else {
            let top_y = area.y;
            let bottom_y = area.bottom();
            let border_color = if self.config.border.color != ratatui::style::Color::Reset {
                self.config.border.color
            } else {
                ratatui::style::Color::DarkGray
            };
            let style = Style::default().fg(border_color);
            (top_y, bottom_y, style)
        };

        let track_height = bottom_y.saturating_sub(top_y) as usize;
        if track_height < 2 || (total_lines <= visible_height && offset == 0) {
            return;
        }

        let scroll_area_size = track_height.saturating_sub(1);
        let (scrollbar_start, scrollbar_height) =
            calc_scrollbar(total_lines, visible_height, offset, scroll_area_size);

        let start_y = top_y + scrollbar_start as u16;
        let end_y = start_y + scrollbar_height as u16;
        let right_x = area.x + area.width.saturating_sub(1);

        let buf = frame.buffer_mut();
        for y in top_y..bottom_y {
            if y >= start_y && y <= end_y {
                if let Some(cell) = buf.cell_mut((right_x, y)) {
                    cell.set_char('▐');
                    cell.set_style(style);
                }
            }
        }
    }
}

#[cfg(unix)]
fn query_tty_picker(timeout: std::time::Duration) -> anyhow::Result<ratatui_image::picker::Picker> {
    use nix::sys::{
        select::{FdSet, select},
        time::{TimeVal, TimeValLike},
    };
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;

    let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;

    let is_tmux = std::env::var("TMUX").is_ok()
        || std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "tmux")
        || std::env::var("TERM").is_ok_and(|t| t.starts_with("tmux"));

    if is_tmux {
        let _ = std::process::Command::new("tmux")
            .args(["set", "-p", "allow-passthrough", "all"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        let query = b"\x1bPtmux;\x1b\x1b_Gi=1,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b\\\x1bPtmux;\x1b\x1b[16t\x1b\\";
        tty.write_all(query)?;
    } else {
        tty.write_all(b"\x1b_Gi=1,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[16t")?;
    }
    tty.flush()?;

    let mut timeout = TimeVal::milliseconds(timeout.as_millis() as i64);
    let mut full_buf = Vec::new();
    loop {
        let mut fds = FdSet::new();
        fds.insert(tty.as_fd());
        let ready = select(None, &mut fds, None, None, Some(&mut timeout))?;
        if ready == 0 {
            break;
        }
        let mut chunk = [0u8; 256];
        let n = tty.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        full_buf.extend_from_slice(&chunk[..n]);
        timeout = TimeVal::milliseconds(15);
    }

    if full_buf.is_empty() {
        if is_tmux || std::env::var("GHOSTTY_RESOURCES_DIR").is_ok() {
            #[allow(deprecated)]
            let mut picker = ratatui_image::picker::Picker::from_fontsize(ratatui_image::FontSize::new(10, 20));
            picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
            let _ = std::process::Command::new("tmux")
                .args(["set", "-p", "allow-passthrough", "all"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            return Ok(picker);
        }
        anyhow::bail!("No response from /dev/tty");
    }

    let resp = String::from_utf8_lossy(&full_buf);
    let is_kitty = resp.contains("_Gi=1;OK") || resp.contains("_Gi=1;EINVAL");

    let mut font_size = None;
    if let Some(pos) = resp.find("\x1b[6;") {
        let rest = &resp[pos + 4..];
        if let Some(end) = rest.find('t') {
            let part = &rest[..end];
            let mut nums = part.split(';');
            if let (Some(h_str), Some(w_str)) = (nums.next(), nums.next()) {
                if let (Ok(h), Ok(w)) = (h_str.parse::<u16>(), w_str.parse::<u16>()) {
                    if w > 0 && h > 0 {
                        font_size = Some(ratatui_image::FontSize::new(w, h));
                    }
                }
            }
        }
    }

    let font_size = font_size.unwrap_or_else(|| ratatui_image::FontSize::new(10, 20));
    let proto = if is_kitty || is_tmux || std::env::var("GHOSTTY_RESOURCES_DIR").is_ok() {
        ratatui_image::picker::ProtocolType::Kitty
    } else {
        ratatui_image::picker::ProtocolType::Halfblocks
    };

    #[allow(deprecated)]
    let mut picker = ratatui_image::picker::Picker::from_fontsize(font_size);
    picker.set_protocol_type(proto);
    if is_tmux {
        let _ = std::process::Command::new("tmux")
            .args(["set", "-p", "allow-passthrough", "all"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(picker)
}

#[cfg(windows)]
fn query_tty_picker(
    _timeout: std::time::Duration,
) -> anyhow::Result<ratatui_image::picker::Picker> {
    anyhow::bail!("TTY querying is not supported on Windows")
}

/// Calculate start offset and height of the scrollbar thumb within `scroll_area_size`.
/// Exact port of lazygit's `calcScrollbar` in `pkg/gocui/scrollbar.go`.
pub fn calc_scrollbar(
    list_size: usize,
    page_size: usize,
    position: usize,
    scroll_area_size: usize,
) -> (usize, usize) {
    let height = calc_scrollbar_height(list_size, page_size, scroll_area_size);
    let max_position = list_size.saturating_sub(page_size);
    if max_position == 0 {
        return (0, height);
    }
    if position >= max_position {
        return (scroll_area_size.saturating_sub(height), height);
    }
    let range = (scroll_area_size.saturating_sub(height).saturating_sub(1)) as f64;
    let start = (((position as f64) / (max_position as f64)) * range).ceil() as usize;
    (start, height)
}

/// Calculate the height of the scrollbar thumb.
/// Exact port of lazygit's `calcScrollbarHeight` in `pkg/gocui/scrollbar.go`.
pub fn calc_scrollbar_height(list_size: usize, page_size: usize, scroll_area_size: usize) -> usize {
    if page_size >= list_size || list_size == 0 {
        return scroll_area_size;
    }

    ((page_size as f64 / list_size as f64) * scroll_area_size as f64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_scrollbar_compatibility() {
        let tests = vec![
            ("page size greater than list size", 5, 10, 0, 20, 0, 20),
            ("page size matches list size", 10, 10, 0, 20, 0, 20),
            ("page size half of list size", 10, 5, 0, 20, 0, 10),
            (
                "page size half of list size at scroll end",
                10,
                5,
                5,
                20,
                10,
                10,
            ),
            (
                "page size third of list size having scrolled half the way",
                15,
                5,
                5,
                21,
                7,
                7,
            ),
            (
                "page size third of list size having scrolled the full way",
                15,
                5,
                10,
                21,
                14,
                7,
            ),
            (
                "page size third of list size having scrolled by one",
                15,
                5,
                1,
                21,
                2,
                7,
            ),
            (
                "page size third of list size having scrolled up from the bottom by one",
                15,
                5,
                9,
                21,
                12,
                7,
            ),
        ];

        for (
            name,
            list_size,
            page_size,
            position,
            scroll_area_size,
            expected_start,
            expected_height,
        ) in tests
        {
            let (start, height) = calc_scrollbar(list_size, page_size, position, scroll_area_size);
            assert_eq!(
                start, expected_start,
                "Test '{}' failed: expected start {}, got {}",
                name, expected_start, start
            );
            assert_eq!(
                height, expected_height,
                "Test '{}' failed: expected height {}, got {}",
                name, expected_height, height
            );
        }
    }

    #[test]
    fn test_render_scrollbar_draws_half_blocks_when_enabled() {
        use crate::preview::previewer::Previewer;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::text::Text;

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut config = PreviewConfig::default();
        config.scrollbar = true;
        config.show = crate::config::ShowCondition::Bool(true);

        let (previewer, _tx) = Previewer::new(Default::default());
        let text: Text<'static> = (1..=30)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        previewer.set_string(text);

        let mut ui = PreviewUI::new(previewer.view(), config, [40, 10]);
        ui.update_dimensions(&Rect::new(0, 0, 40, 10));

        terminal
            .draw(|f| {
                ui.render_scrollbar(f, Rect::new(0, 0, 40, 10));
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let right_x = 39;
        let mut found_thumb = false;
        for y in 0..10 {
            if buf.cell((right_x, y)).map(|c| c.symbol()) == Some("▐") {
                found_thumb = true;
                break;
            }
        }
        assert!(
            found_thumb,
            "Scrollbar thumb '▐' should be rendered on right edge"
        );
    }

    #[test]
    fn test_render_scrollbar_disabled_by_default() {
        use crate::preview::previewer::Previewer;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::text::Text;

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        let config = PreviewConfig::default();
        assert!(
            !config.scrollbar,
            "Preview scrollbar must be disabled by default"
        );

        let (previewer, _tx) = Previewer::new(Default::default());
        let text: Text<'static> = (1..=30)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        previewer.set_string(text);

        let mut ui = PreviewUI::new(previewer.view(), config, [40, 10]);
        ui.update_dimensions(&Rect::new(0, 0, 40, 10));

        terminal
            .draw(|f| {
                ui.render_scrollbar(f, Rect::new(0, 0, 40, 10));
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        for y in 0..10 {
            for x in 0..40 {
                assert_ne!(buf.cell((x, y)).map(|c| c.symbol()), Some("▐"));
            }
        }
    }

    #[test]
    fn test_preview_get_image_state() {
        use crate::preview::previewer::Previewer;
        let config = PreviewConfig {
            media: true,
            ..Default::default()
        };
        let (previewer, _tx) = Previewer::new(Default::default());
        let img = image::DynamicImage::new_rgb8(100, 100);
        previewer.set_image(img);

        let mut ui = PreviewUI::new(previewer.view(), config, [40, 10]);
        let state = ui.get_image_state();
        assert!(state.is_some(), "State must be immediately available synchronously");

        // Test Zoom In (2.0x magnification)
        ui.zoom = 2.0;
        ui.view.image_id.fetch_add(1, std::sync::atomic::Ordering::Release);
        let state_zoomed_in = ui.get_image_state();
        assert!(state_zoomed_in.is_some(), "State must be valid for zoom in");

        // Test Zoom Out (0.5x scaling)
        ui.zoom = 0.5;
        ui.view.image_id.fetch_add(1, std::sync::atomic::Ordering::Release);
        let state_zoomed_out = ui.get_image_state();
        assert!(state_zoomed_out.is_some(), "State must be valid for zoom out");

        // Test dimension update (e.g. entering fullscreen preview)
        ui.update_dimensions(&Rect::new(0, 0, 80, 24));
        let state_resized = ui.get_image_state();
        assert!(state_resized.is_some(), "State must be updated for new area");
    }
}
