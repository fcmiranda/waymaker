use crate::config::{DiagramBackground, DiagramTheme};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::fs;
use std::path::Path;

use super::text::text_to_ansi;

/// Options for Mermaid diagram rendering.
#[derive(Debug, Clone, Default)]
pub struct MermaidOptions {
    /// Column budget for compaction (None = unlimited/natural width).
    pub max_width: Option<usize>,
    /// Whether to force ASCII-only characters (no Unicode box-drawing).
    pub ascii: bool,
    /// Whether to wrap the diagram in a decorative outer box.
    pub show_box: bool,
    /// Optional title to show in the diagram header.
    pub title: Option<String>,
    /// Whether to render inline diagram with Kitty Unicode placeholders when supported.
    pub inline_diagrams: bool,
    /// Diagram theme: Auto, Dark, or Light.
    pub theme: DiagramTheme,
    /// Diagram background mode: Transparent or Solid.
    pub background: DiagramBackground,
}

/// Render a Mermaid diagram source string to Ratatui `Text<'static>`.
pub fn render_mermaid(src: &str, opts: &MermaidOptions) -> Text<'static> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Text::default();
    }

    let render_result = if opts.ascii {
        mermaid_text::render_ascii_with_width(trimmed, opts.max_width)
    } else {
        mermaid_text::render_with_width(trimmed, opts.max_width)
    };

    match render_result {
        Ok(rendered) => style_mermaid_output(&rendered, opts),
        Err(err) => render_mermaid_error(trimmed, &err.to_string(), opts),
    }
}

/// Render a Mermaid diagram to an ANSI colored string for terminal stdout.
pub fn render_mermaid_ansi(src: &str, opts: &MermaidOptions) -> String {
    let text = render_mermaid(src, opts);
    text_to_ansi(&text)
}

/// Read a Mermaid file and render it as Ratatui `Text<'static>`.
pub fn render_mermaid_file(path: &Path, opts: &MermaidOptions) -> anyhow::Result<Text<'static>> {
    let content = fs::read_to_string(path)?;
    Ok(render_mermaid(&content, opts))
}

/// Read a Mermaid file and render it as an ANSI colored string for terminal stdout.
pub fn render_mermaid_file_ansi(path: &Path, opts: &MermaidOptions) -> anyhow::Result<String> {
    let content = fs::read_to_string(path)?;
    Ok(render_mermaid_ansi(&content, opts))
}

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHasher};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

/// Terminal color mode (Dark vs Light).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    Dark,
    Light,
}

/// Omarchy palette colors parsed from `~/.local/state/omarchy/current/theme/colors.toml`.
#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq, Hash)]
pub struct OmarchyColors {
    pub mode: Option<String>,
    pub accent: Option<String>,
    pub selection: Option<String>,
    pub muted: Option<String>,
    pub background: Option<String>,
    pub dark_background: Option<String>,
    pub darker_background: Option<String>,
    pub lighter_background: Option<String>,
    pub foreground: Option<String>,
    pub dark_foreground: Option<String>,
    pub light_foreground: Option<String>,
    pub bright_foreground: Option<String>,
    pub red: Option<String>,
    pub yellow: Option<String>,
    pub orange: Option<String>,
    pub green: Option<String>,
    pub cyan: Option<String>,
    pub blue: Option<String>,
    pub magenta: Option<String>,
    pub brown: Option<String>,
    pub bright_red: Option<String>,
    pub bright_yellow: Option<String>,
    pub bright_green: Option<String>,
    pub bright_cyan: Option<String>,
    pub bright_blue: Option<String>,
    pub bright_magenta: Option<String>,
}

/// Read Omarchy colors from `~/.local/state/omarchy/current/theme/colors.toml` if present.
pub fn read_omarchy_colors() -> Option<OmarchyColors> {
    let home = dirs::home_dir()?;
    let colors_path = home.join(".local/state/omarchy/current/theme/colors.toml");
    let content = fs::read_to_string(colors_path).ok()?;
    toml::from_str(&content).ok()
}

/// Parse terminal background color mode from the standard `$COLORFGBG` environment variable.
/// Format is `fg;bg` or `fg;modifier;bg`.
pub fn parse_colorfgbg(val: &str) -> Option<ColorMode> {
    let trimmed = val.trim();
    if trimmed.eq_ignore_ascii_case("dark") {
        return Some(ColorMode::Dark);
    }
    if trimmed.eq_ignore_ascii_case("light") {
        return Some(ColorMode::Light);
    }
    let parts: Vec<&str> = trimmed.split(';').collect();
    if parts.len() >= 2 {
        let bg_str = parts.last()?.trim();
        if bg_str.eq_ignore_ascii_case("default") {
            let fg_str = parts[0].trim();
            if let Ok(fg_num) = fg_str.parse::<u32>() {
                if fg_num == 0 || fg_num == 8 {
                    return Some(ColorMode::Light);
                } else if fg_num == 7 || fg_num == 15 {
                    return Some(ColorMode::Dark);
                }
            }
        } else if let Ok(bg_num) = bg_str.parse::<u32>() {
            // In ANSI 16-color terminals:
            // 0..=6, 8 are dark backgrounds (black, dark red, green, yellow, blue, magenta, cyan, dark gray)
            // 7 (light gray) and 15 (white) are light backgrounds
            if bg_num == 7 || bg_num == 15 {
                return Some(ColorMode::Light);
            } else if bg_num <= 8 {
                return Some(ColorMode::Dark);
            }
        }
    }
    None
}

/// Detect terminal color mode following the priority order:
/// 1. `~/.local/state/omarchy/current/theme/colors.toml` (`mode = "dark"` / `"light"` or background luminance)
/// 2. `$COLORFGBG` environment variable
/// 3. Fallback: Dark mode default (standard for modern terminals)
pub fn detect_color_mode() -> ColorMode {
    // Priority 1: Omarchy system theme
    if let Some(omarchy) = read_omarchy_colors() {
        if let Some(ref m) = omarchy.mode {
            let ml = m.trim().to_lowercase();
            if ml == "light" {
                return ColorMode::Light;
            } else if ml == "dark" {
                return ColorMode::Dark;
            }
        }
        if let Some(ref bg) = omarchy.background {
            if let Some((r, g, b)) = parse_hex_color(bg) {
                let lum = 0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32);
                if lum > 140.0 {
                    return ColorMode::Light;
                } else {
                    return ColorMode::Dark;
                }
            }
        }
    }

    // Priority 2: COLORFGBG terminal variable
    if let Ok(colorfgbg) = std::env::var("COLORFGBG") {
        if let Some(mode) = parse_colorfgbg(&colorfgbg) {
            return mode;
        }
    }

    // Fallback: Dark mode default
    ColorMode::Dark
}

fn lighten_hex(hex: &str, factor: f32) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return format!("#{hex}");
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let blend = |c: u8| (c as f32 + (255.0 - c as f32) * factor).round() as u8;
    format!("#{:02X}{:02X}{:02X}", blend(r), blend(g), blend(b))
}

fn darken_hex(hex: &str, factor: f32) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return format!("#{hex}");
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let scale = |c: u8| (c as f32 * factor).round() as u8;
    format!("#{:02X}{:02X}{:02X}", scale(r), scale(g), scale(b))
}

fn readable_on(hex: &str) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return "#000000".to_string();
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32;
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if lum > 140.0 {
        "#000000".to_string()
    } else {
        "#FFFFFF".to_string()
    }
}

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let s = hex.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some((r, g, b))
    } else {
        None
    }
}

fn resolve_omarchy_mode(omarchy: &Option<OmarchyColors>) -> Option<ColorMode> {
    let o = omarchy.as_ref()?;
    if let Some(ref m) = o.mode {
        let ml = m.trim().to_lowercase();
        if ml == "light" {
            return Some(ColorMode::Light);
        } else if ml == "dark" {
            return Some(ColorMode::Dark);
        }
    }
    if let Some(ref bg) = o.background {
        if let Some((r, g, b)) = parse_hex_color(bg) {
            let lum = 0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32);
            if lum > 140.0 {
                return Some(ColorMode::Light);
            } else {
                return Some(ColorMode::Dark);
            }
        }
    }
    None
}

/// Compute a 64-bit theme fingerprint taking into account resolved ColorMode,
/// background mode, and active palette colors (Omarchy or fallback).
pub fn compute_theme_fingerprint(theme_opt: DiagramTheme, bg_opt: DiagramBackground) -> u64 {
    let mode = match theme_opt {
        DiagramTheme::Auto => detect_color_mode(),
        DiagramTheme::Dark => ColorMode::Dark,
        DiagramTheme::Light => ColorMode::Light,
    };

    let omarchy = read_omarchy_colors();
    let omarchy_mode = resolve_omarchy_mode(&omarchy);
    let use_omarchy = omarchy.is_some() && omarchy_mode == Some(mode);

    let mut hasher = FxHasher::default();
    (mode as u8).hash(&mut hasher);
    (bg_opt as u8).hash(&mut hasher);
    use_omarchy.hash(&mut hasher);
    if use_omarchy {
        if let Some(ref o) = omarchy {
            o.mode.hash(&mut hasher);
            o.accent.hash(&mut hasher);
            o.background.hash(&mut hasher);
            o.foreground.hash(&mut hasher);
            o.lighter_background.hash(&mut hasher);
            o.selection.hash(&mut hasher);
            o.blue.hash(&mut hasher);
            o.red.hash(&mut hasher);
            o.green.hash(&mut hasher);
            o.yellow.hash(&mut hasher);
            o.magenta.hash(&mut hasher);
            o.cyan.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Construct a `mermaid_rs_renderer::Theme` matching the detected or configured theme mode
/// and background transparency requirements.
pub fn build_mermaid_theme(
    theme_opt: DiagramTheme,
    bg_opt: DiagramBackground,
) -> mermaid_rs_renderer::Theme {
    let mode = match theme_opt {
        DiagramTheme::Auto => detect_color_mode(),
        DiagramTheme::Dark => ColorMode::Dark,
        DiagramTheme::Light => ColorMode::Light,
    };

    let omarchy = read_omarchy_colors();
    let omarchy_mode = resolve_omarchy_mode(&omarchy);
    // Only use Omarchy palette if Omarchy is present AND its color mode matches the target mode!
    let use_omarchy = omarchy.is_some() && omarchy_mode == Some(mode);

    match mode {
        ColorMode::Dark => {
            let mut t = mermaid_rs_renderer::Theme::dark();
            t.background = match bg_opt {
                DiagramBackground::Transparent => "#00000000".to_string(),
                DiagramBackground::Solid => {
                    if use_omarchy {
                        omarchy
                            .as_ref()
                            .and_then(|o| o.background.clone())
                            .unwrap_or_else(|| "#1a1b26".to_string())
                    } else {
                        "#1a1b26".to_string()
                    }
                }
            };

            let (surface, border, fg, muted, dark_bg, yellow) = if use_omarchy {
                let o = omarchy.as_ref().unwrap();
                (
                    o.lighter_background
                        .clone()
                        .or_else(|| o.selection.clone())
                        .unwrap_or_else(|| "#24283b".to_string()),
                    o.accent
                        .clone()
                        .or_else(|| o.blue.clone())
                        .unwrap_or_else(|| "#7aa2f7".to_string()),
                    o.foreground
                        .clone()
                        .unwrap_or_else(|| "#c0caf5".to_string()),
                    o.muted.clone().unwrap_or_else(|| "#565f89".to_string()),
                    o.dark_background
                        .clone()
                        .unwrap_or_else(|| "#13141c".to_string()),
                    o.yellow.clone().unwrap_or_else(|| "#e0af68".to_string()),
                )
            } else {
                (
                    "#24283b".to_string(),
                    "#7aa2f7".to_string(),
                    "#c0caf5".to_string(),
                    "#565f89".to_string(),
                    "#13141c".to_string(),
                    "#e0af68".to_string(),
                )
            };

            t.primary_color = surface.clone();
            t.secondary_color = if use_omarchy {
                omarchy
                    .as_ref()
                    .and_then(|o| o.selection.clone())
                    .unwrap_or_else(|| "#292e42".to_string())
            } else {
                "#292e42".to_string()
            };
            t.tertiary_color = dark_bg.clone();
            t.primary_text_color = fg.clone();
            t.primary_border_color = border.clone();
            t.line_color = border.clone();
            t.edge_label_background = if bg_opt == DiagramBackground::Transparent {
                surface.clone()
            } else if use_omarchy {
                omarchy
                    .as_ref()
                    .and_then(|o| o.background.clone())
                    .unwrap_or_else(|| "#1a1b26".to_string())
            } else {
                "#1a1b26".to_string()
            };
            t.cluster_background = dark_bg.clone();
            t.cluster_border = muted.clone();
            t.text_color = fg.clone();

            t.sequence_actor_fill = surface.clone();
            t.sequence_actor_border = border.clone();
            t.sequence_actor_line = muted.clone();
            t.sequence_note_fill = if use_omarchy {
                omarchy
                    .as_ref()
                    .and_then(|o| o.selection.clone())
                    .unwrap_or_else(|| "#292e42".to_string())
            } else {
                "#292e42".to_string()
            };
            t.sequence_note_border = yellow;
            t.sequence_activation_fill = surface;
            t.sequence_activation_border = border;

            let (red, blue, green, yellow_c, magenta, cyan) = if use_omarchy {
                let o = omarchy.as_ref().unwrap();
                (
                    o.red.as_deref().unwrap_or("#f7768e"),
                    o.blue.as_deref().unwrap_or("#7aa2f7"),
                    o.green.as_deref().unwrap_or("#9ece6a"),
                    o.yellow.as_deref().unwrap_or("#e0af68"),
                    o.magenta.as_deref().unwrap_or("#ad8ee6"),
                    o.cyan.as_deref().unwrap_or("#449dab"),
                )
            } else {
                (
                    "#f7768e", "#7aa2f7", "#9ece6a", "#e0af68", "#ad8ee6", "#449dab",
                )
            };

            t.git_colors = [
                red.to_string(),
                blue.to_string(),
                green.to_string(),
                yellow_c.to_string(),
                magenta.to_string(),
                cyan.to_string(),
                darken_hex(red, 0.65),
                darken_hex(blue, 0.65),
            ];
            t.git_inv_colors = [
                lighten_hex(red, 0.6),
                lighten_hex(blue, 0.6),
                lighten_hex(green, 0.6),
                lighten_hex(yellow_c, 0.6),
                lighten_hex(magenta, 0.6),
                lighten_hex(cyan, 0.6),
                lighten_hex(&darken_hex(red, 0.65), 0.6),
                lighten_hex(&darken_hex(blue, 0.65), 0.6),
            ];
            t.git_branch_label_colors = [
                readable_on(&t.git_colors[0]),
                readable_on(&t.git_colors[1]),
                readable_on(&t.git_colors[2]),
                readable_on(&t.git_colors[3]),
                readable_on(&t.git_colors[4]),
                readable_on(&t.git_colors[5]),
                readable_on(&t.git_colors[6]),
                readable_on(&t.git_colors[7]),
            ];

            t
        }
        ColorMode::Light => {
            let mut t = mermaid_rs_renderer::Theme::modern();
            t.background = match bg_opt {
                DiagramBackground::Transparent => "#00000000".to_string(),
                DiagramBackground::Solid => {
                    if use_omarchy {
                        omarchy
                            .as_ref()
                            .and_then(|o| o.background.clone())
                            .unwrap_or_else(|| "#ffffff".to_string())
                    } else {
                        "#ffffff".to_string()
                    }
                }
            };

            let (surface, border, fg, muted, light_bg, yellow) = if use_omarchy {
                let o = omarchy.as_ref().unwrap();
                (
                    o.lighter_background
                        .clone()
                        .or_else(|| o.selection.clone())
                        .unwrap_or_else(|| "#f1f5f9".to_string()),
                    o.accent
                        .clone()
                        .or_else(|| o.blue.clone())
                        .unwrap_or_else(|| "#2563eb".to_string()),
                    o.foreground
                        .clone()
                        .unwrap_or_else(|| "#0f172a".to_string()),
                    o.muted.clone().unwrap_or_else(|| "#94a3b8".to_string()),
                    o.background
                        .clone()
                        .unwrap_or_else(|| "#ffffff".to_string()),
                    o.yellow.clone().unwrap_or_else(|| "#d97706".to_string()),
                )
            } else {
                (
                    "#f1f5f9".to_string(),
                    "#2563eb".to_string(),
                    "#0f172a".to_string(),
                    "#94a3b8".to_string(),
                    "#ffffff".to_string(),
                    "#d97706".to_string(),
                )
            };

            t.primary_color = surface.clone();
            t.secondary_color = if use_omarchy {
                omarchy
                    .as_ref()
                    .and_then(|o| o.selection.clone())
                    .unwrap_or_else(|| "#e2e8f0".to_string())
            } else {
                "#e2e8f0".to_string()
            };
            t.tertiary_color = light_bg.clone();
            t.primary_text_color = fg.clone();
            t.primary_border_color = border.clone();
            t.line_color = border.clone();
            t.edge_label_background = if bg_opt == DiagramBackground::Transparent {
                surface.clone()
            } else {
                light_bg.clone()
            };
            t.cluster_background = if use_omarchy {
                omarchy
                    .as_ref()
                    .and_then(|o| o.dark_background.clone())
                    .unwrap_or_else(|| "#f8fafc".to_string())
            } else {
                "#f8fafc".to_string()
            };
            t.cluster_border = muted.clone();
            t.text_color = fg.clone();

            t.sequence_actor_fill = surface.clone();
            t.sequence_actor_border = border.clone();
            t.sequence_actor_line = muted.clone();
            t.sequence_note_fill = if use_omarchy {
                omarchy
                    .as_ref()
                    .and_then(|o| o.selection.clone())
                    .unwrap_or_else(|| "#fef3c7".to_string())
            } else {
                "#fef3c7".to_string()
            };
            t.sequence_note_border = yellow;
            t.sequence_activation_fill = surface;
            t.sequence_activation_border = border;

            let (red, blue, green, yellow_c, magenta, cyan) = if use_omarchy {
                let o = omarchy.as_ref().unwrap();
                (
                    o.red.as_deref().unwrap_or("#dc2626"),
                    o.blue.as_deref().unwrap_or("#2563eb"),
                    o.green.as_deref().unwrap_or("#16a34a"),
                    o.yellow.as_deref().unwrap_or("#ca8a04"),
                    o.magenta.as_deref().unwrap_or("#9333ea"),
                    o.cyan.as_deref().unwrap_or("#0891b2"),
                )
            } else {
                (
                    "#dc2626", "#2563eb", "#16a34a", "#ca8a04", "#9333ea", "#0891b2",
                )
            };

            t.git_colors = [
                red.to_string(),
                blue.to_string(),
                green.to_string(),
                yellow_c.to_string(),
                magenta.to_string(),
                cyan.to_string(),
                darken_hex(red, 0.65),
                darken_hex(blue, 0.65),
            ];
            t.git_inv_colors = [
                lighten_hex(red, 0.6),
                lighten_hex(blue, 0.6),
                lighten_hex(green, 0.6),
                lighten_hex(yellow_c, 0.6),
                lighten_hex(magenta, 0.6),
                lighten_hex(cyan, 0.6),
                lighten_hex(&darken_hex(red, 0.65), 0.6),
                lighten_hex(&darken_hex(blue, 0.65), 0.6),
            ];
            t.git_branch_label_colors = [
                readable_on(&t.git_colors[0]),
                readable_on(&t.git_colors[1]),
                readable_on(&t.git_colors[2]),
                readable_on(&t.git_colors[3]),
                readable_on(&t.git_colors[4]),
                readable_on(&t.git_colors[5]),
                readable_on(&t.git_colors[6]),
                readable_on(&t.git_colors[7]),
            ];

            t
        }
    }
}

/// Shared system font database for fast SVG text rasterization.
static FONT_DB: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    Arc::new(db)
});

/// Trigger background warmup of the system font database.
///
/// Moving the 50-650ms `load_system_fonts()` scan to a background thread during
/// startup ensures that the first diagram or SVG preview renders without cold-start UI freeze.
pub fn warmup_font_db() {
    std::thread::Builder::new()
        .name("mm-font-warmup".to_string())
        .spawn(|| {
            let _ = &*FONT_DB;
        })
        .ok();
}

/// In-memory LRU cache for rendered diagram bitmaps, keyed by (FxHash(source), scale_key, theme_fingerprint).
static DIAGRAM_CACHE: LazyLock<
    Mutex<IndexMap<(u64, u32, u64), image::DynamicImage, FxBuildHasher>>,
> = LazyLock::new(|| Mutex::new(IndexMap::with_capacity_and_hasher(64, FxBuildHasher)));

/// Render a Mermaid diagram source string to a `DynamicImage` with theme and background options.
///
/// Uses `mermaid-rs-renderer` to produce SVG with transparent or solid background,
/// then rasterizes with `resvg`/`usvg`/`tiny-skia`.
/// Results are cached in an in-memory LRU cache keyed by diagram hash, scale, and theme fingerprint.
pub fn render_mermaid_to_image_with_options(
    src: &str,
    scale: f32,
    theme_opt: DiagramTheme,
    bg_opt: DiagramBackground,
) -> Option<image::DynamicImage> {
    use mermaid_rs_renderer::{RenderOptions, render_with_options};

    let trimmed = src.trim();
    if trimmed.is_empty() {
        return None;
    }

    let scale_val = scale.max(0.1);
    let scale_key = (scale_val * 100.0) as u32;

    let mut hasher = FxHasher::default();
    trimmed.hash(&mut hasher);
    let hash = hasher.finish();

    let theme_fp = compute_theme_fingerprint(theme_opt, bg_opt);
    let cache_key = (hash, scale_key, theme_fp);

    // Check LRU cache first
    if let Ok(mut cache) = DIAGRAM_CACHE.lock() {
        if let Some((_, img)) = cache.shift_remove_entry(&cache_key) {
            let cloned = img.clone();
            cache.insert(cache_key, img);
            return Some(cloned);
        }
    }

    // Stage 1: Mermaid → SVG with theme
    let theme = build_mermaid_theme(theme_opt, bg_opt);
    let solid_bg = theme.background.clone();
    let render_opts = RenderOptions {
        theme,
        layout: mermaid_rs_renderer::LayoutConfig::default(),
    };
    let svg = render_with_options(trimmed, render_opts).ok()?;

    // Stage 2: SVG → rasterized image via usvg + resvg + tiny-skia
    let img = render_svg_to_image_with_options(&svg, scale_val, bg_opt, Some(&solid_bg))?;

    // Store in LRU cache (evict oldest if over capacity)
    if let Ok(mut cache) = DIAGRAM_CACHE.lock() {
        if cache.len() >= 64 {
            cache.shift_remove_index(0);
        }
        cache.insert(cache_key, img.clone());
    }

    Some(img)
}

/// Render a Mermaid diagram source string to a `DynamicImage` using the graphics pipeline.
/// Defaults to automatic theme detection and transparent background.
pub fn render_mermaid_to_image(src: &str, scale: f32) -> Option<image::DynamicImage> {
    render_mermaid_to_image_with_options(
        src,
        scale,
        DiagramTheme::Auto,
        DiagramBackground::Transparent,
    )
}

/// Rasterize an SVG string to a `DynamicImage` at the given scale factor with background option.
pub fn render_svg_to_image_with_options(
    svg: &str,
    scale: f32,
    bg_opt: DiagramBackground,
    solid_bg_color: Option<&str>,
) -> Option<image::DynamicImage> {
    let opt = usvg::Options {
        fontdb: FONT_DB.clone(),
        ..Default::default()
    };

    let tree = usvg::Tree::from_str(svg, &opt).ok()?;
    let natural = tree.size().to_int_size();

    let w = ((natural.width() as f32) * scale.max(0.1)) as u32;
    let h = ((natural.height() as f32) * scale.max(0.1)) as u32;

    if w == 0 || h == 0 {
        return None;
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;

    // Fill background only if solid is requested; otherwise pixmap remains transparent
    if bg_opt == DiagramBackground::Solid {
        let (r, g, b) = solid_bg_color
            .and_then(parse_hex_color)
            .unwrap_or((26, 27, 38));
        pixmap.fill(resvg::tiny_skia::Color::from_rgba8(r, g, b, 255));
    }

    let transform = resvg::tiny_skia::Transform::from_scale(
        w as f32 / natural.width() as f32,
        h as f32 / natural.height() as f32,
    );

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Convert RGBA pixmap to image::DynamicImage
    let rgba_data = pixmap.data().to_vec();
    let img = image::RgbaImage::from_raw(w, h, rgba_data)?;
    Some(image::DynamicImage::ImageRgba8(img))
}

/// Rasterize an SVG string to a `DynamicImage` at the given scale factor.
/// Defaults to transparent background.
pub fn render_svg_to_image(svg: &str, scale: f32) -> Option<image::DynamicImage> {
    render_svg_to_image_with_options(svg, scale, DiagramBackground::Transparent, None)
}

/// Read a `.mmd`/`.mermaid` file and render it as a `DynamicImage` with theme and background options.
pub fn render_mermaid_file_to_image_with_options(
    path: &Path,
    scale: f32,
    theme_opt: DiagramTheme,
    bg_opt: DiagramBackground,
) -> Option<image::DynamicImage> {
    let content = fs::read_to_string(path).ok()?;
    render_mermaid_to_image_with_options(&content, scale, theme_opt, bg_opt)
}

/// Read a `.mmd`/`.mermaid` file and render it as a `DynamicImage`.
/// Falls back to `None` if graphics rendering fails.
pub fn render_mermaid_file_to_image(path: &Path, scale: f32) -> Option<image::DynamicImage> {
    render_mermaid_file_to_image_with_options(
        path,
        scale,
        DiagramTheme::Auto,
        DiagramBackground::Transparent,
    )
}

/// Encode PNG bytes into a Kitty Graphics Protocol escape sequence string.
/// Handles tmux passthrough wrapping if running inside tmux.
pub fn encode_kitty_graphics(png_bytes: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    let is_tmux = std::env::var("TMUX").is_ok();
    let (start, escape, end) = if is_tmux {
        ("\x1bPtmux;", "\x1b\x1b", "\x1b\\")
    } else {
        ("", "\x1b", "")
    };

    let chunk_size = 4096;
    let mut out = String::new();
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(chunk_size).collect();
    let chunk_count = chunks.len();

    for (i, chunk) in chunks.into_iter().enumerate() {
        let more = if chunk_count > i + 1 { 1 } else { 0 };
        let chunk_str = std::str::from_utf8(chunk).unwrap_or("");
        out.push_str(start);
        if i == 0 {
            out.push_str(&format!(
                "{escape}_Ga=T,f=100,m={more};{chunk_str}{escape}\\"
            ));
        } else {
            out.push_str(&format!("{escape}_Gm={more};{chunk_str}{escape}\\"));
        }
        out.push_str(end);
    }
    out.push('\n');
    out
}

/// Render a Mermaid diagram source string directly to a Kitty Graphics Protocol escape sequence.
///
/// Returns `None` if rendering or PNG encoding fails.
pub fn render_mermaid_to_kitty(src: &str, scale: f32) -> Option<String> {
    let img = render_mermaid_to_image(src, scale)?;
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .ok()?;

    Some(encode_kitty_graphics(&png_bytes))
}

/// Unicode placeholder base character defined in the Kitty Graphics Protocol specification.
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// Diacritics lookup table (256 entries) for encoding row, column, and image ID in Unicode placeholders.
pub const DIACRITICS: &[&str] = &[
    "0305", "030D", "030E", "0310", "0312", "033D", "033E", "033F", "0346", "034A", "034B", "034C",
    "0350", "0351", "0352", "0357", "035B", "0363", "0364", "0365", "0366", "0367", "0368", "0369",
    "036A", "036B", "036C", "036D", "036E", "036F", "0483", "0484", "0485", "0486", "0487", "0592",
    "0593", "0594", "0595", "0597", "0598", "0599", "059C", "059D", "059E", "059F", "05A0", "05A1",
    "05A8", "05A9", "05AB", "05AC", "05AF", "05C4", "0610", "0611", "0612", "0613", "0614", "0615",
    "0616", "0617", "0657", "0658", "0659", "065A", "065B", "065D", "065E", "06D6", "06D7", "06D8",
    "06D9", "06DA", "06DB", "06DC", "06DF", "06E0", "06E1", "06E2", "06E4", "06E7", "06E8", "06EB",
    "06EC", "0730", "0732", "0733", "0735", "0736", "073A", "073D", "073F", "0740", "0741", "0743",
    "0745", "0747", "0749", "074A", "07EB", "07EC", "07ED", "07EE", "07EF", "07F0", "07F1", "07F3",
    "0816", "0817", "0818", "0819", "081B", "081C", "081D", "081E", "081F", "0820", "0821", "0822",
    "0823", "0825", "0826", "0827", "0829", "082A", "082B", "082C", "082D", "0951", "0953", "0954",
    "0F82", "0F83", "0F86", "0F87", "135D", "135E", "135F", "17DD", "193A", "1A17", "1A75", "1A76",
    "1A77", "1A78", "1A79", "1A7A", "1A7B", "1A7C", "1B6B", "1B6D", "1B6E", "1B6F", "1B70", "1B71",
    "1B72", "1B73", "1CD0", "1CD1", "1CD2", "1CDA", "1CDB", "1CE0", "1DC0", "1DC1", "1DC3", "1DC4",
    "1DC5", "1DC6", "1DC7", "1DC8", "1DC9", "1DCB", "1DCC", "1DD1", "1DD2", "1DD3", "1DD4", "1DD5",
    "1DD6", "1DD7", "1DD8", "1DD9", "1DDA", "1DDB", "1DDC", "1DDD", "1DDE", "1DDF", "1DE0", "1DE1",
    "1DE2", "1DE3", "1DE4", "1DE5", "1DE6", "1DFE", "20D0", "20D1", "20D4", "20D5", "20D6", "20D7",
    "20DB", "20DC", "20E1", "20E7", "20E9", "20F0", "2CEF", "2CF0", "2CF1", "2DE0", "2DE1", "2DE2",
    "2DE3", "2DE4", "2DE5", "2DE6", "2DE7", "2DE8", "2DE9", "2DEA", "2DEB", "2DEC", "2DED", "2DEE",
    "2DEF", "2DF0", "2DF1", "2DF2", "2DF3", "2DF4", "2DF5", "2DF6", "2DF7", "2DF8", "2DF9", "2DFA",
    "2DFB", "2DFC", "2DFD", "2DFE", "2DFF", "A66F", "A67C", "A67D", "A6F0", "A6F1", "A8E0", "A8E1",
    "A8E2", "A8E3", "A8E4", "A8E5", "A8E6", "A8E7", "A8E8", "A8E9", "A8EA", "A8EB", "A8EC", "A8ED",
    "A8EE", "A8EF", "A8F0", "A8F1", "AAB0", "AAB2", "AAB3", "AAB7", "AAB8", "AABE", "AABF", "AAC1",
    "FE20", "FE21", "FE22", "FE23", "FE24", "FE25", "FE26", "10A0F", "10A38", "1D185", "1D186",
    "1D187", "1D188", "1D189", "1D1AA", "1D1AB", "1D1AC", "1D1AD", "1D242", "1D243", "1D244",
];

static DIACRITIC_CHARS: LazyLock<Vec<char>> = LazyLock::new(|| {
    DIACRITICS
        .iter()
        .filter_map(|hex| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
        .collect()
});

/// Retrieve the combining diacritic character for the given 0..255 index.
#[inline]
pub fn get_diacritic(index: usize) -> Option<char> {
    DIACRITIC_CHARS.get(index).copied()
}

fn detect_kitty_support() -> bool {
    // 1. Ghostty environment markers
    if std::env::var("GHOSTTY_RESOURCES_DIR").is_ok()
        || std::env::var("GHOSTTY_BIN_DIR").is_ok()
        || std::env::var("GHOSTTY_SURFACE_ID").is_ok()
    {
        return true;
    }

    // 2. Kitty environment markers
    if std::env::var("KITTY_PID").is_ok() || std::env::var("KITTY_WINDOW_ID").is_ok() {
        return true;
    }

    // 3. TERM environment variable
    if let Ok(term) = std::env::var("TERM") {
        let t = term.to_ascii_lowercase();
        if t.contains("kitty") || t.contains("ghostty") {
            return true;
        }
    }

    // 4. TERM_PROGRAM environment variable
    let is_prog_supported = |prog: &str| {
        let p = prog.to_ascii_lowercase();
        p.contains("ghostty") || p.contains("kitty")
    };

    if let Ok(prog) = std::env::var("TERM_PROGRAM") {
        if is_prog_supported(&prog) {
            return true;
        }
    }

    // 5. In tmux, query outer terminal from tmux client or global environment
    if std::env::var("TMUX").is_ok() {
        if let Ok(output) = std::process::Command::new("tmux")
            .args(["display-message", "-p", "#{client_termname}"])
            .output()
        {
            if output.status.success() {
                let term = String::from_utf8_lossy(&output.stdout).to_lowercase();
                if term.contains("ghostty") || term.contains("kitty") {
                    return true;
                }
            }
        }

        if let Ok(output) = std::process::Command::new("tmux")
            .args(["show-environment", "-g", "TERM_PROGRAM"])
            .output()
        {
            if output.status.success() {
                let out_str = String::from_utf8_lossy(&output.stdout).to_lowercase();
                if out_str.contains("ghostty") || out_str.contains("kitty") {
                    return true;
                }
            }
        }
    }

    false
}

static DETECTED_KITTY_SUPPORT: LazyLock<bool> = LazyLock::new(detect_kitty_support);

/// Check whether the current terminal emulator natively supports Kitty Graphics Protocol
/// based purely on environment detection, without any diagram-specific overrides.
pub fn is_kitty_terminal_supported() -> bool {
    *DETECTED_KITTY_SUPPORT
}

/// Check whether the current terminal environment supports Kitty Graphics Protocol and Unicode Placeholders.
///
/// Strictly supports terminals verified to implement Kitty Unicode Placeholders (`\u{10EEEE}`)
/// with combining diacritics (Kitty and Ghostty). Terminals without Unicode placeholder support
/// (like WezTerm or legacy Alacritty) fall back gracefully to text preview.
pub fn is_kitty_supported() -> bool {
    // 1. Explicit user override via environment variable has top precedence (WM_INLINE_DIAGRAMS or legacy MM_INLINE_DIAGRAMS)
    if let Ok(val) = std::env::var("WM_INLINE_DIAGRAMS").or_else(|_| std::env::var("MM_INLINE_DIAGRAMS")) {
        let v = val.trim();
        if v == "0"
            || v.eq_ignore_ascii_case("false")
            || v.eq_ignore_ascii_case("off")
            || v.eq_ignore_ascii_case("no")
        {
            return false;
        }
        if v == "1"
            || v.eq_ignore_ascii_case("true")
            || v.eq_ignore_ascii_case("on")
            || v.eq_ignore_ascii_case("yes")
        {
            return true;
        }
    }

    *DETECTED_KITTY_SUPPORT
}

/// Rendered Kitty Unicode Placeholder diagram representation.
#[derive(Debug, Clone)]
pub struct KittyPlaceholderDiagram {
    pub image_id: u32,
    pub cols: u32,
    pub rows: u32,
    pub transmission: String,
    pub lines: Vec<Line<'static>>,
}

/// Encode PNG image bytes into Kitty Graphics escape sequences using Unicode Placeholder mode (`U=1`).
/// Wrapped in Tmux DCS passthrough (`\x1bPtmux;...`) when running inside tmux.
///
/// Follows the Kitty protocol specification: first chunk transmits full control headers,
/// subsequent chunks specify solely `m` and `q` keys.
pub fn encode_kitty_unicode_transmission(
    png_bytes: &[u8],
    image_id: u32,
    cols: u32,
    rows: u32,
) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    let is_tmux = std::env::var("TMUX").is_ok();
    let (prefix, suffix) = if is_tmux {
        ("\x1bPtmux;\x1b\x1b_G", "\x1b\x1b\\\x1b\\")
    } else {
        ("\x1b_G", "\x1b\\")
    };

    let chunk_size = 4096;
    let mut out = String::new();
    let total_bytes = b64.len();
    let mut start = 0;

    while start < total_bytes {
        let end = std::cmp::min(start + chunk_size, total_bytes);
        let chunk_data = &b64[start..end];
        let more_chunks = (end != total_bytes) as u8;

        if start == 0 {
            out.push_str(&format!(
                "{prefix}a=T,f=100,i={image_id},c={cols},r={rows},U=1,q=2,m={more_chunks};{chunk_data}{suffix}"
            ));
        } else {
            out.push_str(&format!("{prefix}m={more_chunks},q=2;{chunk_data}{suffix}"));
        }

        start = end;
    }

    out
}

static TRANSMITTED_IMAGES: LazyLock<Mutex<std::collections::HashSet<u32>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

#[cfg(not(test))]
static TMUX_PASSTHROUGH_INIT: std::sync::Once = std::sync::Once::new();

/// Ensure tmux allows escape sequence passthrough for Kitty graphics.
pub fn ensure_tmux_passthrough() {
    #[cfg(not(test))]
    {
        if std::env::var("TMUX").is_ok() {
            TMUX_PASSTHROUGH_INIT.call_once(|| {
                let status = std::process::Command::new("tmux")
                    .args(["set", "-p", "allow-passthrough", "all"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                if status.map(|s| !s.success()).unwrap_or(true) {
                    let _ = std::process::Command::new("tmux")
                        .args(["set", "-p", "allow-passthrough", "on"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            });
        }
    }
}

/// Transmit raw escape sequences to the terminal directly via /dev/tty or stdout.
pub fn transmit_kitty_raw(seq: &str) {
    #[cfg(test)]
    {
        let _ = seq;
        return;
    }

    #[cfg(not(test))]
    {
        ensure_tmux_passthrough();

        use std::io::Write;
        if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
            let _ = tty.write_all(seq.as_bytes());
            let _ = tty.flush();
        } else {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(seq.as_bytes());
            let _ = out.flush();
        }
    }
}

/// Transmit Kitty Graphics escape sequences idempotently (only once per unique image ID).
pub fn transmit_kitty_image_idempotent(image_id: u32, transmission: &str) {
    if let Ok(mut set) = TRANSMITTED_IMAGES.lock() {
        if set.contains(&image_id) {
            return;
        }
        set.insert(image_id);
    }
    transmit_kitty_raw(transmission);
}

/// Encode a Kitty Graphics Protocol deletion escape sequence for a specific image ID (`a=d,d=i,i=...`).
pub fn encode_kitty_delete(image_id: u32) -> String {
    let is_tmux = std::env::var("TMUX").is_ok();
    let (prefix, suffix) = if is_tmux {
        ("\x1bPtmux;\x1b\x1b_G", "\x1b\x1b\\\x1b\\")
    } else {
        ("\x1b_G", "\x1b\\")
    };
    format!("{prefix}a=d,d=i,i={image_id}{suffix}")
}

/// Delete a Kitty Graphics image from the terminal GPU texture cache by ID.
pub fn delete_kitty_image(image_id: u32) {
    if let Ok(mut set) = TRANSMITTED_IMAGES.lock() {
        set.remove(&image_id);
    }
    transmit_kitty_raw(&encode_kitty_delete(image_id));
}

/// Encode a Kitty Graphics Protocol deletion escape sequence for all images (`a=d,d=a`).
pub fn encode_kitty_delete_all() -> String {
    let is_tmux = std::env::var("TMUX").is_ok()
        || std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "tmux")
        || std::env::var("TERM").is_ok_and(|t| t.starts_with("tmux"));
    let (prefix, suffix) = if is_tmux {
        ("\x1bPtmux;\x1b\x1b_G", "\x1b\x1b\\\x1b\\")
    } else {
        ("\x1b_G", "\x1b\\")
    };
    format!("{prefix}a=d,d=a{suffix}")
}

/// Delete all Kitty Graphics images from the terminal screen and GPU cache.
pub fn delete_kitty_all() {
    if let Ok(mut set) = TRANSMITTED_IMAGES.lock() {
        set.clear();
    }
    transmit_kitty_raw(&encode_kitty_delete_all());
}

static NEXT_DIAGRAM_IMAGE_ID: AtomicU32 = AtomicU32::new(1001);

/// Generate the next unique image ID for Kitty diagram transmissions.
pub fn next_diagram_image_id() -> u32 {
    NEXT_DIAGRAM_IMAGE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Create Ratatui Lines containing Unicode Placeholders for the given image ID and dimensions.
///
/// Follows Kitty specification: 24-bit image ID is encoded in foreground RGB `(r, g, b)`.
/// Diacritic 1 encodes row, diacritic 2 encodes column. The 3rd diacritic is emitted only
/// when the high byte `(image_id >> 24)` is non-zero, avoiding redundant bytes and parser ambiguity.
pub fn create_unicode_placeholder_lines(
    image_id: u32,
    cols: u32,
    rows: u32,
    indent: usize,
) -> Vec<Line<'static>> {
    let r = ((image_id >> 16) & 0xFF) as u8;
    let g = ((image_id >> 8) & 0xFF) as u8;
    let b = (image_id & 0xFF) as u8;
    let fg = Color::Rgb(r, g, b);
    let style = Style::default().fg(fg);

    let id_high_byte = ((image_id >> 24) & 0xFF) as usize;
    let id_high_char = if id_high_byte > 0 {
        get_diacritic(id_high_byte)
    } else {
        None
    };

    let pad_span = if indent > 0 {
        Some(Span::raw(" ".repeat(indent)))
    } else {
        None
    };

    let max_diacritic_idx = DIACRITIC_CHARS.len().saturating_sub(1);
    let clamped_rows = (rows as usize).min(max_diacritic_idx);
    let clamped_cols = (cols as usize).min(max_diacritic_idx);

    let mut lines = Vec::with_capacity(clamped_rows);

    for row in 0..clamped_rows {
        let row_char = get_diacritic(row);
        let mut row_str = String::with_capacity(clamped_cols * 8);

        for col in 0..clamped_cols {
            row_str.push(PLACEHOLDER);
            if let Some(rd) = row_char {
                row_str.push(rd);
            }
            if let Some(cd) = get_diacritic(col) {
                row_str.push(cd);
            }
            if let Some(hd) = id_high_char {
                row_str.push(hd);
            }
        }

        let mut spans = Vec::new();
        if let Some(ref p) = pad_span {
            spans.push(p.clone());
        }
        spans.push(Span::styled(row_str, style));
        lines.push(Line::from(spans));
    }

    lines
}

/// Wrap Unicode placeholder lines in an aesthetic decorative top/bottom border frame.
///
/// Guarantees that the top banner (`╭─ [mermaid: inline diagram] ... ╮`) and the bottom
/// border (`╰ ... ╯`) have the exact same character width, avoiding visual misalignment.
#[deprecated(note = "Inline diagrams flow borderlessly with clean blank line spacing")]
pub fn frame_unicode_placeholder_lines(lines: Vec<Line<'static>>, cols: u32) -> Vec<Line<'static>> {
    let border_style = Style::default().fg(Color::Cyan);
    let prefix = "╭─ [mermaid: inline diagram] ";
    let p_len = UnicodeWidthStr::width(prefix);
    let total_width = (cols as usize + 4).max(p_len + 2);
    let fill_count = total_width.saturating_sub(p_len + 1);

    let mut top = String::with_capacity(total_width * 3);
    top.push_str(prefix);
    top.push_str(&"─".repeat(fill_count));
    top.push('╮');

    let mut bot = String::with_capacity(total_width * 3);
    bot.push('╰');
    bot.push_str(&"─".repeat(total_width.saturating_sub(2)));
    bot.push('╯');

    let mut framed = Vec::with_capacity(lines.len() + 3);
    framed.push(Line::from(Span::styled(top, border_style)));
    framed.extend(lines);
    framed.push(Line::from(Span::styled(bot, border_style)));
    framed.push(Line::default());
    framed
}

/// In-memory LRU cache for rendered Kitty Unicode placeholder diagrams, keyed by (FxHash(source), cols, rows, theme_fingerprint).
static PLACEHOLDER_CACHE: LazyLock<
    Mutex<IndexMap<(u64, u32, u32, u64), KittyPlaceholderDiagram, FxBuildHasher>>,
> = LazyLock::new(|| Mutex::new(IndexMap::with_capacity_and_hasher(64, FxBuildHasher)));

/// Render a Mermaid diagram source to Kitty Unicode Placeholders (`\u{10EEEE}`) with theme and background options.
///
/// Utilizes the LRU rasterization cache in `render_mermaid_to_image_with_options` and `PLACEHOLDER_CACHE`,
/// computes target cell grid dimensions, encodes PNG and transmission sequence on cache misses,
/// and generates Ratatui `Line`s with the exact placeholder grapheme clusters.
/// Returns `None` if graphics rasterization fails (enabling graceful fallback).
pub fn render_mermaid_to_unicode_placeholders_with_options(
    src: &str,
    max_width: Option<usize>,
    theme_opt: DiagramTheme,
    bg_opt: DiagramBackground,
) -> Option<KittyPlaceholderDiagram> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut hasher = FxHasher::default();
    trimmed.hash(&mut hasher);
    let hash = hasher.finish();

    // 1. Render to DynamicImage via cached pipeline (scale 1.5 for crisp vector rendering)
    let img = render_mermaid_to_image_with_options(trimmed, 1.5, theme_opt, bg_opt)?;

    // 2. Query actual terminal cell pixel dimensions if available
    let (cell_w, cell_h) = match crossterm::terminal::window_size() {
        Ok(ws) if ws.columns > 0 && ws.rows > 0 && ws.width > 0 && ws.height > 0 => (
            ws.width as f32 / ws.columns as f32,
            ws.height as f32 / ws.rows as f32,
        ),
        _ => (10.0, 20.0),
    };

    let img_w = img.width() as f32;
    let img_h = img.height() as f32;

    if img_w <= 0.0 || img_h <= 0.0 {
        return None;
    }

    // 3. Compute column and row budget
    let natural_cols = (img_w / cell_w).ceil() as u32;
    let budget_width = max_width.unwrap_or(80).saturating_sub(4).max(10) as u32;
    let max_diacritics = (DIACRITIC_CHARS.len().saturating_sub(1) as u32).min(255);
    let cols = natural_cols.min(budget_width).clamp(10, max_diacritics);

    let aspect = img_h / img_w;
    let natural_rows = (((cols as f32) * cell_w * aspect) / cell_h).round() as u32;
    let rows = natural_rows.clamp(3, 80.min(max_diacritics));

    let theme_fp = compute_theme_fingerprint(theme_opt, bg_opt);
    let cache_key = (hash, cols, rows, theme_fp);

    // 4. Check LRU placeholder cache first
    if let Ok(mut cache) = PLACEHOLDER_CACHE.lock() {
        if let Some((_, diag)) = cache.shift_remove_entry(&cache_key) {
            let cloned = diag.clone();
            cache.insert(cache_key, diag);
            return Some(cloned);
        }
    }

    // 5. Encode image to PNG (only executed on cache miss)
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .ok()?;

    // 6. Generate distinct image ID
    let image_id = NEXT_DIAGRAM_IMAGE_ID.fetch_add(1, Ordering::Relaxed);

    // 7. Build transmission sequence
    let transmission = encode_kitty_unicode_transmission(&png_bytes, image_id, cols, rows);

    // 8. Generate placeholder lines (indent 0 for clean borderless text flow)
    let indent = 0;
    let lines = create_unicode_placeholder_lines(image_id, cols, rows, indent);

    let diag = KittyPlaceholderDiagram {
        image_id,
        cols,
        rows,
        transmission,
        lines,
    };

    // 9. Store in LRU cache (evict oldest if over capacity)
    if let Ok(mut cache) = PLACEHOLDER_CACHE.lock() {
        if cache.len() >= 64 {
            cache.shift_remove_index(0);
        }
        cache.insert(cache_key, diag.clone());
    }

    Some(diag)
}

/// Render a Mermaid diagram source to Kitty Unicode Placeholders (`\u{10EEEE}`).
/// Defaults to automatic theme detection and transparent background.
pub fn render_mermaid_to_unicode_placeholders(
    src: &str,
    max_width: Option<usize>,
) -> Option<KittyPlaceholderDiagram> {
    render_mermaid_to_unicode_placeholders_with_options(
        src,
        max_width,
        DiagramTheme::Auto,
        DiagramBackground::Transparent,
    )
}

use unicode_width::UnicodeWidthStr;

/// Style the plain-text diagram output with theme colors for box borders, connectors, and labels.
fn style_mermaid_output(rendered: &str, opts: &MermaidOptions) -> Text<'static> {
    let raw_lines: Vec<&str> = rendered.lines().collect();
    if raw_lines.is_empty() {
        return Text::default();
    }

    let mut lines = Vec::new();

    let mode = match opts.theme {
        DiagramTheme::Auto => detect_color_mode(),
        DiagramTheme::Dark => ColorMode::Dark,
        DiagramTheme::Light => ColorMode::Light,
    };
    let (border_color, text_color) = match mode {
        ColorMode::Dark => (Color::Cyan, Color::White),
        ColorMode::Light => (Color::Blue, Color::Black),
    };
    let border_style = Style::default().fg(border_color);
    let text_style = Style::default().fg(text_color).add_modifier(Modifier::BOLD);

    let content_width = raw_lines
        .iter()
        .map(|l| UnicodeWidthStr::width(*l))
        .max()
        .unwrap_or(0);

    let title_str = opts.title.as_deref().unwrap_or("Mermaid Diagram");
    let title_w = UnicodeWidthStr::width(title_str);

    let inner_width = content_width.max(title_w + 4);
    let box_width = if let Some(mw) = opts.max_width {
        inner_width.min(mw.saturating_sub(4).max(inner_width.min(20)))
    } else {
        inner_width
    };

    if opts.show_box {
        if opts.ascii {
            let mut top = format!("+-[ {title_str} ]");
            let remaining = (box_width + 4).saturating_sub(top.len() + 1);
            top.push_str(&"-".repeat(remaining));
            top.push('+');
            lines.push(Line::from(Span::styled(top, border_style)));
        } else {
            let prefix = format!("╭─ [{title_str}] ");
            let p_len = UnicodeWidthStr::width(prefix.as_str());
            let fill_count = (box_width + 4).saturating_sub(p_len + 1);
            let mut top = prefix;
            top.push_str(&"─".repeat(fill_count));
            top.push('╮');
            lines.push(Line::from(Span::styled(
                top,
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )));
        }
    }

    for raw_line in raw_lines {
        let line_w = UnicodeWidthStr::width(raw_line);
        let mut spans = Vec::new();
        let mut current_text = String::new();
        let mut current_is_box = false;

        for ch in raw_line.chars() {
            let is_box = is_diagram_glyph(ch, opts.ascii);
            if spans.is_empty() && current_text.is_empty() {
                current_is_box = is_box;
                current_text.push(ch);
            } else if is_box == current_is_box {
                current_text.push(ch);
            } else {
                let style = if current_is_box {
                    border_style
                } else {
                    text_style
                };
                spans.push(Span::styled(std::mem::take(&mut current_text), style));
                current_is_box = is_box;
                current_text.push(ch);
            }
        }

        if !current_text.is_empty() {
            let style = if current_is_box {
                border_style
            } else {
                text_style
            };
            spans.push(Span::styled(current_text, style));
        }

        if opts.show_box {
            let pad = box_width.saturating_sub(line_w);
            let pad_str = " ".repeat(pad);
            let left_border = if opts.ascii { "| " } else { "│ " };
            let right_border = if opts.ascii { " |" } else { " │" };
            let border_color = if opts.ascii { Color::Cyan } else { Color::Blue };
            let side_style = Style::default().fg(border_color);

            let mut boxed_spans = vec![Span::styled(left_border, side_style)];
            boxed_spans.extend(spans);
            if !pad_str.is_empty() {
                boxed_spans.push(Span::raw(pad_str));
            }
            boxed_spans.push(Span::styled(right_border, side_style));
            lines.push(Line::from(boxed_spans));
        } else {
            lines.push(Line::from(spans));
        }
    }

    if opts.show_box {
        if opts.ascii {
            let mut bot = String::from("+");
            bot.push_str(&"-".repeat(box_width + 2));
            bot.push('+');
            lines.push(Line::from(Span::styled(bot, border_style)));
        } else {
            let mut bot = String::from("╰");
            bot.push_str(&"─".repeat(box_width + 2));
            bot.push('╯');
            lines.push(Line::from(Span::styled(
                bot,
                Style::default().fg(Color::Blue),
            )));
        }
    }

    Text::from(lines)
}

/// Check if a character is a diagram border, connector, or edge glyph.
fn is_diagram_glyph(ch: char, ascii: bool) -> bool {
    if ascii {
        return matches!(
            ch,
            '+' | '-' | '|' | '>' | '<' | '^' | 'v' | '\\' | '/' | '*'
        );
    }
    matches!(
        ch,
        '─' | '│'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
            | '╭'
            | '╮'
            | '╯'
            | '╰'
            | '═'
            | '║'
            | '╒'
            | '╕'
            | '╘'
            | '╛'
            | '╞'
            | '╡'
            | '╤'
            | '╧'
            | '╪'
            | '╔'
            | '╗'
            | '╚'
            | '╝'
            | '╠'
            | '╣'
            | '╦'
            | '╩'
            | '╬'
            | '▲'
            | '▼'
            | '◄'
            | '►'
            | '▸'
            | '▾'
            | '▴'
            | '◀'
            | '▶'
            | '△'
            | '▽'
            | '▷'
            | '◁'
            | '◂'
            | '◆'
            | '◇'
            | '○'
            | '●'
            | '╱'
            | '╲'
            | '┄'
            | '┆'
            | '┈'
            | '┊'
            | '╌'
            | '╎'
            | '╍'
            | '╏'
            | '╴'
            | '╵'
            | '╶'
            | '╷'
            | '█'
            | '░'
            | '▒'
            | '▓'
    )
}

/// Render a diagram syntax error gracefully with a warning badge and the raw source.
fn render_mermaid_error(src: &str, error_msg: &str, opts: &MermaidOptions) -> Text<'static> {
    let mut lines = Vec::new();
    let err_border = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);

    let err_str = format!("Error: {error_msg}");
    let raw_lines: Vec<&str> = src.lines().collect();
    let content_w = err_str.len().max(
        raw_lines
            .iter()
            .map(|l| UnicodeWidthStr::width(*l))
            .max()
            .unwrap_or(0),
    );
    let box_width = content_w.max(34);

    if opts.ascii {
        let mut top = String::from("+-[ Mermaid Syntax Warning ]");
        let fill = (box_width + 4).saturating_sub(top.len() + 1);
        top.push_str(&"-".repeat(fill));
        top.push('+');
        lines.push(Line::from(Span::styled(top, err_border)));

        let pad_err = box_width.saturating_sub(err_str.len());
        lines.push(Line::from(vec![
            Span::styled("| ", err_border),
            Span::styled(err_str, Style::default().fg(Color::LightRed)),
            Span::raw(" ".repeat(pad_err)),
            Span::styled(" |", err_border),
        ]));

        let mut mid = String::from("+-[ Source ]");
        let fill_mid = (box_width + 4).saturating_sub(mid.len() + 1);
        mid.push_str(&"-".repeat(fill_mid));
        mid.push('+');
        lines.push(Line::from(Span::styled(mid, err_border)));

        for src_line in raw_lines {
            let line_w = UnicodeWidthStr::width(src_line);
            let pad = box_width.saturating_sub(line_w);
            lines.push(Line::from(vec![
                Span::styled("| ", err_border),
                Span::styled(src_line.to_string(), Style::default().fg(Color::DarkGray)),
                Span::raw(" ".repeat(pad)),
                Span::styled(" |", err_border),
            ]));
        }

        let mut bot = String::from("+");
        bot.push_str(&"-".repeat(box_width + 2));
        bot.push('+');
        lines.push(Line::from(Span::styled(bot, err_border)));
    } else {
        let mut top = String::from("╭─ [Mermaid Syntax Warning] ");
        let fill = (box_width + 4).saturating_sub(top.len() + 1);
        top.push_str(&"─".repeat(fill));
        top.push('╮');
        lines.push(Line::from(Span::styled(top, err_border)));

        let pad_err = box_width.saturating_sub(err_str.len());
        lines.push(Line::from(vec![
            Span::styled("│ ", err_border),
            Span::styled(err_str, Style::default().fg(Color::LightRed)),
            Span::raw(" ".repeat(pad_err)),
            Span::styled(" │", err_border),
        ]));

        let mut mid = String::from("├─ [Source] ");
        let fill_mid = (box_width + 4).saturating_sub(mid.len() + 1);
        mid.push_str(&"─".repeat(fill_mid));
        mid.push('┤');
        lines.push(Line::from(Span::styled(mid, err_border)));

        for src_line in raw_lines {
            let line_w = UnicodeWidthStr::width(src_line);
            let pad = box_width.saturating_sub(line_w);
            lines.push(Line::from(vec![
                Span::styled("│ ", err_border),
                Span::styled(src_line.to_string(), Style::default().fg(Color::DarkGray)),
                Span::raw(" ".repeat(pad)),
                Span::styled(" │", err_border),
            ]));
        }

        let mut bot = String::from("╰");
        bot.push_str(&"─".repeat(box_width + 2));
        bot.push('╯');
        lines.push(Line::from(Span::styled(bot, err_border)));
    }

    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_flowchart_basic() {
        let src = "graph TD\n    A[Start] --> B[End]";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
        assert!(!text.lines.is_empty());
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
        assert!(joined.contains("Start"));
        assert!(joined.contains("End"));
    }

    #[test]
    fn test_render_ascii_mode() {
        let src = "graph LR\n    A[Left] --> B[Right]";
        let mut opts = MermaidOptions::default();
        opts.ascii = true;
        let text = render_mermaid(src, &opts);
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
        assert!(joined.contains("Left"));
        assert!(joined.contains("Right"));
        assert!(joined.is_ascii());
    }

    #[test]
    fn test_render_width_constrained() {
        let src = "graph LR\n    A[Start] --> B[Process] --> C[Done]";
        let mut opts = MermaidOptions::default();
        opts.max_width = Some(40);
        let text = render_mermaid(src, &opts);
        assert!(!text.lines.is_empty());
    }

    #[test]
    fn test_render_error_fallback() {
        let src = "this is not valid mermaid @#$*&";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
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
        assert!(joined.contains("Mermaid Syntax Warning"));
        assert!(joined.contains("this is not valid mermaid"));
    }

    #[test]
    fn test_render_empty() {
        let src = "   \n\t  ";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
        assert!(text.lines.is_empty());
    }

    #[test]
    fn test_render_ansi() {
        let src = "graph TD\n    A[NodeA] --> B[NodeB]";
        let opts = MermaidOptions::default();
        let ansi = render_mermaid_ansi(src, &opts);
        assert!(ansi.contains("NodeA"));
        assert!(ansi.contains("NodeB"));
    }

    #[test]
    fn test_render_ascii_with_box_is_pure_ascii() {
        let src = "graph LR\n    A[Step 1] --> B[Step 2]";
        let opts = MermaidOptions {
            ascii: true,
            show_box: true,
            title: Some("ASCII Box Test".to_string()),
            ..Default::default()
        };
        let text = render_mermaid(src, &opts);
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
            joined.is_ascii(),
            "Rendered ASCII diagram with box must be pure ASCII"
        );
        assert!(joined.contains("+-[ ASCII Box Test ]"));
        assert!(joined.contains("Step 1"));
        assert!(joined.contains("Step 2"));
    }

    #[test]
    fn test_render_box_symmetric_width() {
        let src =
            "graph LR\n    Alpha[Start Here] --> Beta[A Somewhat Longer Step] --> Gamma[Done]";
        let opts = MermaidOptions {
            show_box: true,
            title: Some("Symmetry Test".to_string()),
            ..Default::default()
        };
        let text = render_mermaid(src, &opts);
        let widths: Vec<usize> = text
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum()
            })
            .collect();
        assert!(!widths.is_empty());
        let expected_w = widths[0];
        for (idx, w) in widths.iter().enumerate() {
            assert_eq!(
                *w, expected_w,
                "Line {idx} width ({w}) does not match header width ({expected_w})"
            );
        }
    }

    #[test]
    fn test_render_class_diagram() {
        let src = "classDiagram\n    Animal <|-- Duck";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
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
        assert!(joined.contains("Animal"));
        assert!(joined.contains("Duck"));
    }

    #[test]
    fn test_render_sequence_diagram() {
        let src = "sequenceDiagram\n    Alice->>Bob: Ping\n    Bob-->>Alice: Pong";
        let opts = MermaidOptions::default();
        let text = render_mermaid(src, &opts);
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
        assert!(joined.contains("Alice"));
        assert!(joined.contains("Bob"));
        assert!(joined.contains("Ping"));
        assert!(joined.contains("Pong"));
    }

    #[test]
    fn test_render_mermaid_to_image() {
        let src =
            "flowchart TD\n    A[Start] --> B{Choice}\n    B -->|Yes| C[OK]\n    B -->|No| D[Stop]";
        let img = render_mermaid_to_image(src, 1.0);
        assert!(
            img.is_some(),
            "Should successfully render flowchart to image"
        );
        let img = img.unwrap();
        assert!(img.width() > 0, "Image width must be positive");
        assert!(img.height() > 0, "Image height must be positive");

        // Test zoom / scaling
        let img_scaled = render_mermaid_to_image(src, 2.0).unwrap();
        assert!(
            img_scaled.width() > img.width(),
            "Scaled image width should be larger: {} vs {}",
            img_scaled.width(),
            img.width()
        );

        // Test LRU cache hit (should return identical dimensions instantly)
        let cached = render_mermaid_to_image(src, 1.0).unwrap();
        assert_eq!(cached.width(), img.width());
        assert_eq!(cached.height(), img.height());
    }

    #[test]
    fn test_render_mermaid_to_image_empty() {
        assert!(render_mermaid_to_image("", 1.0).is_none());
        assert!(render_mermaid_to_image("   \n  ", 1.0).is_none());
    }

    #[test]
    fn test_git_worktree_diagram() {
        let src = r#"graph TD
    BARE[".dotfiles/.bare (Git Bare Repository)"] --> MAIN[".dotfiles/main (Production Worktree / Stowed)"]
    BARE --> WT1[".dotfiles/feat-zsh-perf (AI Sandbox 1)"]
    BARE --> WT2[".dotfiles/feat-nvim-ui (AI Sandbox 2)"]
    
    MAIN ===|./stow.sh| HOME["$HOME (~/.config, ~/.zsh, ~/.local/bin, etc.)"]
    WT1 -.->|ISOLATED / NOT STOWED| HOME
    WT2 -.->|ISOLATED / NOT STOWED| HOME"#;
        let img = render_mermaid_to_image(src, 1.0);
        println!(
            "Render result: {:?}",
            img.as_ref().map(|i| (i.width(), i.height()))
        );
        assert!(img.is_some(), "Worktree diagram should render to image");
    }

    #[test]
    fn test_diacritics_table_coverage() {
        assert!(
            DIACRITICS.len() >= 256,
            "DIACRITICS table must have at least 256 entries"
        );
        for i in 0..DIACRITICS.len() {
            assert!(
                get_diacritic(i).is_some(),
                "Index {i} must yield a valid diacritic char"
            );
        }
    }

    #[test]
    fn test_encode_kitty_unicode_transmission_format() {
        let fake_png = b"FAKE_PNG_BINARY_DATA_FOR_TESTING";
        let seq = encode_kitty_unicode_transmission(fake_png, 42, 60, 20);
        assert!(seq.contains("a=T"), "Must specify action transmit");
        assert!(seq.contains("f=100"), "Must specify format 100 (PNG)");
        assert!(seq.contains("i=42"), "Must include image ID");
        assert!(seq.contains("c=60"), "Must include columns");
        assert!(seq.contains("r=20"), "Must include rows");
        assert!(seq.contains("U=1"), "Must enable Unicode placeholder mode");
        assert!(seq.contains("q=2"), "Must set quiet mode");
    }

    #[test]
    fn test_encode_kitty_delete_format() {
        let del = encode_kitty_delete(42);
        assert!(del.contains("a=d"), "Must specify action delete");
        assert!(del.contains("d=i"), "Must specify delete by image ID");
        assert!(del.contains("i=42"), "Must include image ID 42");
    }

    #[test]
    fn test_encode_kitty_delete_all_format() {
        let del = encode_kitty_delete_all();
        assert!(del.contains("a=d"), "Must specify action delete");
        assert!(del.contains("d=a"), "Must specify delete all images");
    }

    #[test]
    fn test_create_unicode_placeholder_lines() {
        let lines = create_unicode_placeholder_lines(12345, 10, 5, 2);
        assert_eq!(lines.len(), 5, "Must create exactly 5 lines for rows=5");
        for line in lines {
            let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(joined.starts_with("  "), "Must have 2-space indent");
            assert!(
                joined.contains(PLACEHOLDER),
                "Must contain placeholder character"
            );
            let count = joined.matches(PLACEHOLDER).count();
            assert_eq!(count, 10, "Must have exactly 10 placeholders for cols=10");
        }
    }

    #[test]
    fn test_render_mermaid_to_unicode_placeholders_success() {
        let src = "graph LR\n  A[Start] --> B[End]";
        let diag = render_mermaid_to_unicode_placeholders(src, Some(60));
        assert!(
            diag.is_some(),
            "Diagram should successfully rasterize to placeholders"
        );
        let d = diag.unwrap();
        assert!(d.cols >= 10);
        assert!(d.rows >= 3);
        assert_eq!(d.lines.len(), d.rows as usize);
        assert!(d.transmission.contains("U=1"));
    }

    #[test]
    #[allow(deprecated)]
    fn test_frame_unicode_placeholder_lines_alignment() {
        for cols in [5, 10, 20, 40, 80] {
            let lines = create_unicode_placeholder_lines(12345, cols, 3, 2);
            let framed = frame_unicode_placeholder_lines(lines, cols);
            assert!(framed.len() >= 5);
            let top_str = framed[0].spans[0].content.as_ref();
            let bot_str = framed[framed.len() - 2].spans[0].content.as_ref();
            let top_width = UnicodeWidthStr::width(top_str);
            let bot_width = UnicodeWidthStr::width(bot_str);
            assert_eq!(
                top_width, bot_width,
                "Top and bottom border widths must be equal for cols={cols}: top={top_width}, bot={bot_width}"
            );
        }
    }

    #[test]
    fn test_placeholder_lru_cache_hit() {
        let src = "graph LR\n  X[Alpha] --> Y[Beta]";
        let diag1 = render_mermaid_to_unicode_placeholders(src, Some(60)).expect("First render");
        let diag2 = render_mermaid_to_unicode_placeholders(src, Some(60)).expect("Second render");
        assert_eq!(
            diag1.image_id, diag2.image_id,
            "Cached diagram must retain stable image_id without generating new ones"
        );
        assert_eq!(diag1.cols, diag2.cols);
        assert_eq!(diag1.rows, diag2.rows);
        assert_eq!(diag1.lines.len(), diag2.lines.len());
    }

    #[test]
    fn test_multichunk_transmission_spec_compliance() {
        // Generate payload larger than 4096 base64 bytes (> 3072 raw bytes)
        let large_bytes = vec![0xABu8; 6000];
        let seq = encode_kitty_unicode_transmission(&large_bytes, 777, 50, 25);
        let chunks: Vec<&str> = seq.split("\x1b\\").filter(|s| !s.is_empty()).collect();
        assert!(chunks.len() >= 2, "Must be split into at least 2 chunks");

        // Chunk 0 must specify a=T, f=100, i=777, c=50, r=25, U=1
        assert!(chunks[0].contains("a=T"));
        assert!(chunks[0].contains("i=777"));
        assert!(chunks[0].contains("m=1"));

        // Subsequent chunks must NOT repeat i= or a=
        for sub in &chunks[1..] {
            assert!(
                !sub.contains("i=777"),
                "Subsequent chunk must not repeat i= id"
            );
            assert!(
                !sub.contains("a=T"),
                "Subsequent chunk must not repeat a= action"
            );
            assert!(
                sub.contains("m="),
                "Subsequent chunk must specify m= chunk marker"
            );
        }
    }

    #[test]
    fn test_diacritics_count_for_24bit_vs_32bit_id() {
        // Standard 24-bit ID: only 2 diacritics per cell (row, col)
        let id_24bit = 0x001234;
        let lines_24 = create_unicode_placeholder_lines(id_24bit, 1, 1, 0);
        let cell_24 = lines_24[0].spans[0].content.as_ref();
        let chars_24: Vec<char> = cell_24.chars().collect();
        assert_eq!(
            chars_24.len(),
            3,
            "24-bit ID must have exactly 1 base + 2 diacritics (row, col)"
        );
        assert_eq!(chars_24[0], PLACEHOLDER);

        // 32-bit ID (MSB > 0): 3 diacritics per cell (row, col, msb)
        let id_32bit = 0x02001234;
        let lines_32 = create_unicode_placeholder_lines(id_32bit, 1, 1, 0);
        let cell_32 = lines_32[0].spans[0].content.as_ref();
        let chars_32: Vec<char> = cell_32.chars().collect();
        assert_eq!(
            chars_32.len(),
            4,
            "32-bit ID with MSB > 0 must have 1 base + 3 diacritics"
        );
    }

    #[test]
    fn test_is_kitty_supported_escape_hatch() {
        let prev_wm = std::env::var("WM_INLINE_DIAGRAMS").ok();
        let prev_mm = std::env::var("MM_INLINE_DIAGRAMS").ok();
        unsafe {
            std::env::remove_var("WM_INLINE_DIAGRAMS");
            std::env::remove_var("MM_INLINE_DIAGRAMS");
        }

        unsafe {
            std::env::set_var("WM_INLINE_DIAGRAMS", "0");
        }
        assert!(
            !is_kitty_supported(),
            "WM_INLINE_DIAGRAMS=0 must disable support regardless of other vars"
        );

        unsafe {
            std::env::set_var("WM_INLINE_DIAGRAMS", "1");
        }
        assert!(
            is_kitty_supported(),
            "WM_INLINE_DIAGRAMS=1 must force enable support"
        );

        unsafe {
            std::env::remove_var("WM_INLINE_DIAGRAMS");
            std::env::set_var("MM_INLINE_DIAGRAMS", "0");
        }
        assert!(
            !is_kitty_supported(),
            "MM_INLINE_DIAGRAMS=0 fallback must disable support"
        );

        unsafe {
            std::env::set_var("MM_INLINE_DIAGRAMS", "1");
        }
        assert!(
            is_kitty_supported(),
            "MM_INLINE_DIAGRAMS=1 fallback must force enable support"
        );

        unsafe {
            match prev_wm {
                Some(v) => std::env::set_var("WM_INLINE_DIAGRAMS", v),
                None => std::env::remove_var("WM_INLINE_DIAGRAMS"),
            }
            match prev_mm {
                Some(v) => std::env::set_var("MM_INLINE_DIAGRAMS", v),
                None => std::env::remove_var("MM_INLINE_DIAGRAMS"),
            }
        }
    }

    #[test]
    fn test_parse_colorfgbg() {
        assert_eq!(parse_colorfgbg("15;0"), Some(ColorMode::Dark));
        assert_eq!(parse_colorfgbg("7;0"), Some(ColorMode::Dark));
        assert_eq!(parse_colorfgbg("0;7"), Some(ColorMode::Light));
        assert_eq!(parse_colorfgbg("0;15"), Some(ColorMode::Light));
        assert_eq!(parse_colorfgbg("15;default;0"), Some(ColorMode::Dark));
        assert_eq!(parse_colorfgbg("0;default;15"), Some(ColorMode::Light));
        assert_eq!(parse_colorfgbg("15;default"), Some(ColorMode::Dark));
        assert_eq!(parse_colorfgbg("0;default"), Some(ColorMode::Light));
        assert_eq!(parse_colorfgbg("dark"), Some(ColorMode::Dark));
        assert_eq!(parse_colorfgbg("DARK"), Some(ColorMode::Dark));
        assert_eq!(parse_colorfgbg("light"), Some(ColorMode::Light));
        assert_eq!(parse_colorfgbg("invalid"), None);
    }

    #[test]
    fn test_build_mermaid_theme_transparency_and_modes() {
        // Transparent dark theme
        let dark_transparent =
            build_mermaid_theme(DiagramTheme::Dark, DiagramBackground::Transparent);
        assert_eq!(
            dark_transparent.background, "#00000000",
            "Transparent background must be hex #00000000"
        );
        assert!(!dark_transparent.primary_text_color.is_empty());

        // Solid dark theme
        let dark_solid = build_mermaid_theme(DiagramTheme::Dark, DiagramBackground::Solid);
        assert_ne!(
            dark_solid.background, "#00000000",
            "Solid background must not be transparent"
        );

        // Transparent light theme
        let light_transparent =
            build_mermaid_theme(DiagramTheme::Light, DiagramBackground::Transparent);
        assert_eq!(light_transparent.background, "#00000000");

        // Solid light theme
        let light_solid = build_mermaid_theme(DiagramTheme::Light, DiagramBackground::Solid);
        assert_ne!(light_solid.background, "#00000000");
    }

    #[test]
    fn test_build_mermaid_theme_omarchy_mismatch_protection() {
        // Even if Omarchy has dark mode colors active on the current machine,
        // an explicit Light theme request must NOT use dark mode's navy background or light text.
        let light_solid = build_mermaid_theme(DiagramTheme::Light, DiagramBackground::Solid);
        assert_eq!(
            light_solid.background, "#ffffff",
            "Light solid background must be white, not dark navy from Omarchy"
        );
        assert_eq!(
            light_solid.primary_text_color, "#0f172a",
            "Light primary text color must be dark high-contrast (#0f172a)"
        );

        let light_trans = build_mermaid_theme(DiagramTheme::Light, DiagramBackground::Transparent);
        assert_eq!(light_trans.background, "#00000000");
        assert_eq!(
            light_trans.primary_text_color, "#0f172a",
            "Light transparent primary text color must be dark high-contrast"
        );
    }

    #[test]
    fn test_compute_theme_fingerprint_isolation() {
        let fp_dark_trans =
            compute_theme_fingerprint(DiagramTheme::Dark, DiagramBackground::Transparent);
        let fp_dark_solid = compute_theme_fingerprint(DiagramTheme::Dark, DiagramBackground::Solid);
        let fp_light_trans =
            compute_theme_fingerprint(DiagramTheme::Light, DiagramBackground::Transparent);
        let fp_light_solid =
            compute_theme_fingerprint(DiagramTheme::Light, DiagramBackground::Solid);

        assert_ne!(fp_dark_trans, fp_dark_solid);
        assert_ne!(fp_dark_trans, fp_light_trans);
        assert_ne!(fp_light_trans, fp_light_solid);
        assert_ne!(fp_dark_solid, fp_light_solid);
    }

    #[test]
    fn test_render_mermaid_to_image_transparent_pixels() {
        let src = "graph LR\n  A[Foo] --> B[Bar]";
        let img = render_mermaid_to_image_with_options(
            src,
            1.0,
            DiagramTheme::Dark,
            DiagramBackground::Transparent,
        );
        assert!(img.is_some(), "Diagram should successfully render to image");
        let dynamic = img.unwrap();
        let rgba = dynamic.to_rgba8();

        // Corner pixel (0, 0) should be transparent (alpha == 0)
        let corner_pixel = rgba.get_pixel(0, 0);
        assert_eq!(
            corner_pixel[3], 0,
            "Corner pixel of transparent diagram must have alpha == 0, got rgba: {:?}",
            corner_pixel
        );

        // Render solid diagram and verify corner pixel is opaque
        let solid_img = render_mermaid_to_image_with_options(
            src,
            1.0,
            DiagramTheme::Dark,
            DiagramBackground::Solid,
        );
        assert!(solid_img.is_some());
        let solid_rgba = solid_img.unwrap().to_rgba8();
        let solid_corner = solid_rgba.get_pixel(0, 0);
        assert_eq!(
            solid_corner[3], 255,
            "Corner pixel of solid diagram must have alpha == 255, got rgba: {:?}",
            solid_corner
        );
    }

    #[test]
    fn test_diagram_cache_differentiation_by_theme_and_bg() {
        let src = "graph TD\n  Start --> Finish";
        let img_trans = render_mermaid_to_image_with_options(
            src,
            1.0,
            DiagramTheme::Dark,
            DiagramBackground::Transparent,
        )
        .unwrap();
        let img_solid = render_mermaid_to_image_with_options(
            src,
            1.0,
            DiagramTheme::Dark,
            DiagramBackground::Solid,
        )
        .unwrap();

        // Pixel (0, 0) alpha must differ between transparent and solid
        assert_eq!(img_trans.to_rgba8().get_pixel(0, 0)[3], 0);
        assert_eq!(img_solid.to_rgba8().get_pixel(0, 0)[3], 255);
    }

    #[test]
    fn test_diagram_theme_and_bg_enums() {
        assert_eq!("auto".parse::<DiagramTheme>().unwrap(), DiagramTheme::Auto);
        assert_eq!("dark".parse::<DiagramTheme>().unwrap(), DiagramTheme::Dark);
        assert_eq!(
            "light".parse::<DiagramTheme>().unwrap(),
            DiagramTheme::Light
        );
        assert_eq!(
            "system".parse::<DiagramTheme>().unwrap(),
            DiagramTheme::Auto
        );
        assert!("invalid".parse::<DiagramTheme>().is_err());

        assert_eq!(
            "transparent".parse::<DiagramBackground>().unwrap(),
            DiagramBackground::Transparent
        );
        assert_eq!(
            "solid".parse::<DiagramBackground>().unwrap(),
            DiagramBackground::Solid
        );
        assert_eq!(
            "none".parse::<DiagramBackground>().unwrap(),
            DiagramBackground::Transparent
        );
        assert_eq!(
            "opaque".parse::<DiagramBackground>().unwrap(),
            DiagramBackground::Solid
        );
        assert!("invalid".parse::<DiagramBackground>().is_err());

        assert_eq!(DiagramTheme::Dark.to_string(), "dark");
        assert_eq!(DiagramBackground::Transparent.to_string(), "transparent");
    }
}
