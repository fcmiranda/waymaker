/// Apply a single fzf-style `--color key:value[,key:value…]` spec to config.
///
/// Supported keys and the config paths they map onto:
///
/// | Key              | Config path                                   |
/// |------------------|-----------------------------------------------|
/// | `fg`             | `render.results.style.fg`                     |
/// | `bg`             | `render.results.style.bg`                     |
/// | `hl-fg`          | `render.results.current_style.fg`             |
/// | `hl-bg`          | `render.results.current_style.bg`             |
/// | `border`         | `render.ui.border.color`                      |
/// | `label`          | `render.ui.border.title_fg`                   |
/// | `title`          | `render.ui.border.title_fg`                   |
/// | `preview-border` | `render.preview.border.color`                 |
/// | `preview-label`  | `render.preview.border.title_fg`              |
/// | `preview-title`  | `render.preview.border.title_fg`              |
/// | `preview-border-title` | `render.preview.border.title_fg`         |
/// | `list-border`    | `render.results.border.color`                 |
/// | `list-label`     | `render.results.border.title_fg`              |
/// | `list-title`     | `render.results.border.title_fg`              |
/// | `input-border`   | `render.query.border.color`                   |
/// | `input-label`    | `render.query.border.title_fg`                |
/// | `input-title`    | `render.query.border.title_fg`                |
/// | `header-border`  | `render.header.border.color`                  |
/// | `header-label`   | `render.header.border.title_fg`               |
/// | `header-title`   | `render.header.border.title_fg`               |
/// | `group-header`   | `render.results.group_header_style.fg`        |
/// | `nav`            | `render.ui.nav.color`                         |
/// | `selected-fg`    | `render.results.selected_style.fg`            |
/// | `selected-bg`    | `render.results.selected_style.bg`            |
/// | `selected-prefix`| `render.results.selected_prefix_style.fg`     |
/// | `unselected-prefix`| `render.results.unselected_prefix_style.fg`   |
/// | `spinner`        | `render.results.spinner_style.fg`             |
/// | `yank`           | `render.results.yank_prefix_style.fg`         |
/// | `cut`            | `render.results.cut_prefix_style.fg`          |
/// | `symlink`        | `render.results.symlink.style.fg`             |
/// | `tier-separator` | `render.results.tier.style.fg`                |
use std::str::FromStr;

use ratatui::style::Color;

use crate::config::Config;

pub fn apply_color_spec(config: &mut Config, spec: &str) {
    for pair in spec.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((key, value)) = pair.split_once(':') else {
            log::warn!("--color: expected 'key:value', got '{pair}'");
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let Ok(color) = Color::from_str(value) else {
            log::warn!("--color: unknown color value '{value}' for key '{key}'");
            continue;
        };

        match key {
            "fg" => config.render.results.style.fg = Some(color),
            "bg" => config.render.results.style.bg = Some(color),
            "hl-fg" | "current-fg" => config.render.results.current_style.fg = Some(color),
            "hl-bg" | "current-bg" => config.render.results.current_style.bg = Some(color),
            "border" => config.render.ui.border.color = color,
            "label" | "title" => config.render.ui.border.title_fg = color,
            "preview-border" => config.render.preview.border.color = color,
            "preview-label" | "preview-title" | "preview-border-title" => {
                config.render.preview.border.title_fg = color
            }
            "list-border" => config.render.results.border.color = color,
            "list-label" | "list-title" => config.render.results.border.title_fg = color,
            "input-border" => config.render.query.border.color = color,
            "input-label" | "input-title" => config.render.query.border.title_fg = color,
            "header-border" => config.render.header.border.color = color,
            "header-label" | "header-title" => config.render.header.border.title_fg = color,
            "group-header" => config.render.results.group_header_style.fg = Some(color),
            "nav" => config.render.ui.nav.color = color,
            "selected-fg" => config.render.results.selected_style.fg = Some(color),
            "selected-bg" => config.render.results.selected_style.bg = Some(color),
            "selected-prefix" => config.render.results.selected_prefix_style.fg = Some(color),
            "unselected-prefix" => config.render.results.unselected_prefix_style.fg = Some(color),
            "spinner" => config.render.results.spinner_style.fg = Some(color),
            "yank" => config.render.results.yank_prefix_style.fg = Some(color),
            "cut" => config.render.results.cut_prefix_style.fg = Some(color),
            "symlink" => config.render.results.symlink.style.fg = Some(color),
            "tier-separator" => config.render.results.tier.style.fg = Some(color),
            _ => log::warn!("--color: unknown key '{key}'"),
        }
    }
}
