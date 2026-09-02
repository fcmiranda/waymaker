use std::{
    collections::HashMap,
    env::set_current_dir,
    io::{IsTerminal, Read},
    path::Path,
    process::{Command, Stdio, exit},
    sync::{Arc, Mutex},
};

use crate::{
    action::{ActionContext, MMAction, action_handler},
    clap::Cli,
    color::apply_color_spec,
    config::PartialConfig,
    paths::{last_key_path, presets_path},
    register::MMExt,
    utils::{expand_tilde, guess_clip_cmd, guess_editor_cmd, guess_pager_cmd},
};
use crate::{config::Config, paths::default_config_path};
use cba::{
    _wbog,
    bait::{OptionExt, ResultExt, TransformExt},
    bo::{MapReaderError, map_chunks, map_reader_lines, read_to_chunks, write_str},
    bog::BogOkExt,
    ebog, ibog, prints, wbog,
};
use cba::{bo::load_type, broc::CommandExt};
use log::debug;
use matchmaker::{
    Action, ConfigInjector, MatchError, Matchmaker, OddEnds, PickOptions, SSS, acs,
    binds::{BindMap, BindMapExt},
    config::{BlinkRate, CommandSetting, EnvValue, MatcherConfig, StartConfig},
    event::{EventLoop, RenderSender},
    make_previewer,
    message::{Event, Interrupt},
    nucleo::{
        ColumnIndexable,
        injector::{AnsiInjector, Either, IndexedInjector, Injector, SegmentedInjector},
    },
    preview::AppendOnly,
    render::MMState,
    use_formatter,
};
use matchmaker_partial::Apply;

pub fn enter(cli: Cli, partial: PartialConfig) -> anyhow::Result<Config> {
    if cli.test_keys {
        super::crokey::main();
        exit(0);
    }

    let cfg_path = if let Some(p) = &cli.config {
        Path::new(p)
    } else {
        default_config_path()
    };

    if cli.dump_config && std::io::stdout().is_terminal() {
        // if stdout: dump the default cfg with comments
        write_str(cfg_path, crate::config::DEFAULT_CONFIG)?;
        ibog!("Config written to {cfg_path:?}");
        exit(0)
    }

    #[cfg(debug_assertions)]
    if cli.config.is_none() {
        #[cfg(target_os = "windows")]
        write_str(cfg_path, include_str!("../assets/win.dev.toml")).unwrap();

        #[cfg(not(target_os = "windows"))]
        write_str(cfg_path, include_str!("../assets/dev.toml")).unwrap();
    }

    let mut config: Config = if cli.config.is_some() {
        // Explicit --config: load file verbatim, no default layering.
        load_type(cfg_path, |s| toml::from_str(s))._ebog().or_exit()
    } else if cfg_path.exists() {
        // Default user config path: overlay the file on top of the embedded
        // defaults so the user only needs to specify what they want to change
        // (including individual [binds] entries).
        let mut base = Config::default();
        let user: PartialConfig = load_type(cfg_path, |s| toml::from_str(s))._ebog().or_exit();
        base.apply(user);
        base
    } else {
        Config::default()
    };
    // check config
    if config.source.is_some() {
        wbog!("'source' field is not supported in the main config.");
    }

    if config.render.status.template.is_empty() {
        config.render.status.template = r#"\m/\t"#.to_string();
    }

    // apply overrides
    for mut p in cli.r#override {
        if p.is_relative() && p.extension().is_none() {
            let main_p = presets_path().join(&p).join("main.toml");
            p = if !main_p.exists() {
                presets_path().join(p.with_extension("toml"))
            } else {
                main_p
            };
        }
        // no recursion because tail bad
        let o: PartialConfig = load_type(&p, |s| toml::from_str(s))?;

        if let Some(q) = &o.source {
            let source = p.parent().as_ref().unwrap().join(q);
            let o: PartialConfig = load_type(source, |s| toml::from_str(s))?;
            if o.source.is_some() {
                _wbog!("Ignoring 'source' field in nested override.");
            }
            config.apply(o);
        }

        config.apply(o);
        config.envs.insert(
            "MM_OVERRIDE".to_string(),
            EnvValue::new(p.to_string_lossy().to_string()),
        );
    }

    if config.start.command.base_command.is_none() {
        config.start.command.base_command = Some(config.start.command.command.clone());
    }

    // Apply matching directory / path rules (from config.toml and loaded presets)
    let current_dir = std::env::current_dir().unwrap_or_default();
    let rules = config.rule.clone();
    for rule in &rules {
        if rule.path.matches(&current_dir) {
            if let Some(ref p) = rule.preset {
                let mut preset_path = p.clone();
                if preset_path.is_relative() && preset_path.extension().is_none() {
                    let main_p = presets_path().join(&preset_path).join("main.toml");
                    preset_path = if !main_p.exists() {
                        presets_path().join(preset_path.with_extension("toml"))
                    } else {
                        main_p
                    };
                }
                if let Ok(preset_config) = load_type(&preset_path, |s| toml::from_str(s)) {
                    config.apply(preset_config);
                }
            }
            config.apply(rule.override_config.clone());
        }
    }

    if !config.folder_rules.is_empty() {
        config
            .render
            .ui
            .folder_rules
            .extend(config.folder_rules.clone());
    }
    if config.default_sort.is_some() {
        config.render.ui.default_sort = config.default_sort;
    }

    #[cfg(debug_assertions)]
    {
        config.tui.clear_on_exit = false;
    }
    config.apply(partial); // resolve config.exit first

    if !cli.args.is_empty() {
        if !std::io::stdin().is_terminal() && !cli.no_read {
            eprintln!(
                "warning: trailing arguments provided but input is piped. ignoring trailing arguments."
            );
        }
        *COMMAND_ARGS.lock().unwrap() = cli.args;
    }

    // dispatch subcommands
    if cli.last_key {
        let path = config
            .exit
            .last_key_path
            .as_deref()
            .unwrap_or(last_key_path());

        let content = std::fs::read_to_string(path)._elog();
        if let Some(s) = content
            && let s = s.trim()
            && !s.is_empty()
        {
            prints!(s);
            exit(0);
        } else {
            exit(1)
        }
    }

    if cli.fullscreen {
        config.tui.layout = None;
    }

    if cli.sort {
        config.start.sort = true;
    }

    if cli.frecency {
        config.matcher.worker.frecency = true;
    }

    if cli.icons {
        config.render.results.icons = true;
    }

    if cli.symlink_target {
        config.render.results.symlink_target = true;
    }

    if let Some(pos) = cli.pos {
        config.render.results.pos = Some(pos);
    }

    if let Some(props) = &cli.media {
        apply_media_props(props, &mut config);
    }

    for spec in &cli.color {
        apply_color_spec(&mut config, spec);
    }

    if let Some(nav) = &cli.nav {
        apply_nav_props(nav, &mut config);
    }

    if cli.nav_hints {
        config.render.ui.nav_hints = true;
    }

    if cli.parent_peek {
        config.render.ui.parent_peek.enabled = true;
    }

    if cli.status_inline {
        config.render.query.status_inline = true;
    }

    for nb in &cli.nav_bind {
        if let Some(colon) = nb.find(':') {
            let key = nb[..colon].to_string();
            let action_str = &nb[colon + 1..];
            let parts = split_nav_bind_actions(action_str);

            let mut actions = matchmaker::action::Actions::default();
            let mut parse_ok = true;
            for part in &parts {
                match part.parse::<matchmaker::action::Action<matchmaker::action::NullActionExt>>()
                {
                    Ok(action) => actions.push(action),
                    Err(e) => {
                        eprintln!("warning: invalid --nav-bind action '{}': {}", part, e);
                        parse_ok = false;
                        break;
                    }
                }
            }

            if parse_ok && !actions.is_empty() {
                config.render.ui.nav_binds.insert(key, actions);
            }
        } else {
            eprintln!(
                "warning: --nav-bind '{}' missing ':' separator (expected char:Action)",
                nb
            );
        }
    }

    if config.render.ui.nav_mode && !config.render.ui.nav_basic {
        use matchmaker::action::Actions;
        let mut nb = |k: &str, actions: Actions<matchmaker::action::NullActionExt>| {
            config
                .render
                .ui
                .nav_binds
                .entry(k.to_string())
                .or_insert(actions);
        };
        nb("d", matchmaker::acs![Action::Semantic("fm_delete".into())]);
        nb("a", matchmaker::acs![Action::Semantic("fm_create".into())]);
        nb("r", matchmaker::acs![Action::Semantic("fm_rename".into())]);
        nb("z", matchmaker::acs![Action::Semantic("fm_zip".into())]);
        nb("Z", matchmaker::acs![Action::Semantic("fm_unzip".into())]);
        nb(" ", matchmaker::acs![Action::Toggle]);
        nb("y", matchmaker::acs![Action::Semantic("fm_yank".into())]);
        nb("Y", matchmaker::acs![Action::Semantic("fm_unyank".into())]);
        nb("x", matchmaker::acs![Action::Semantic("fm_cut".into())]);
        nb("X", matchmaker::acs![Action::Semantic("fm_uncut".into())]);
        nb("p", matchmaker::acs![Action::Semantic("fm_paste".into())]);
        nb("u", matchmaker::acs![Action::Semantic("fm_undo".into())]);
        nb(
            "ctrl-r",
            matchmaker::acs![Action::Semantic("fm_redo".into())],
        );
        nb(
            "D",
            matchmaker::acs![Action::Semantic("fm_dragdrop".into())],
        );
        nb(",", matchmaker::acs![Action::SortMenu]);
        nb("f", matchmaker::acs![Action::Semantic("reloadnext".into())]);
        nb("b", matchmaker::acs![Action::Semantic("pins".into())]);
        nb("*", matchmaker::acs![Action::Semantic("pin".into())]);
    }

    if cli.dump_config {
        let contents = toml::to_string_pretty(&config).expect("failed to serialize to TOML");

        // if piped: dump the current cfg
        std::io::Write::write_all(&mut std::io::stdout(), contents.as_bytes())?;

        exit(0);
    }

    // check binds
    config.binds = BindMap::default_binds().modify(|x| x.extend(config.binds));
    if config.render.ui.nav_mode {
        config.binds.insert(
            "tab".parse().expect("tab trigger should parse"),
            matchmaker::acs![matchmaker::Action::ToggleFocus],
        );
        config.binds.insert(
            "shift-enter".parse().expect("shift-enter should parse"),
            matchmaker::acs![
                matchmaker::Action::Print("{=}".to_string()),
                matchmaker::Action::Quit(2)
            ],
        );
    }

    let mut def_sem = |trigger_str: &str, action: Action<MMAction>| {
        if let Ok(t) = trigger_str.parse() {
            config.binds.entry(t).or_insert(matchmaker::acs![action]);
        }
    };
    def_sem("@dirs", Action::Custom(MMAction::ReloadNext(Some(1))));
    def_sem("@frecency", Action::Custom(MMAction::ReloadNext(Some(1))));
    def_sem("@bookmarks", Action::Custom(MMAction::ReloadNext(Some(2))));
    def_sem("@bookmark", Action::Custom(MMAction::FmTogglePin));
    def_sem("@pin", Action::Custom(MMAction::FmTogglePin));
    def_sem("@pins", Action::Custom(MMAction::ReloadNext(Some(2))));
    def_sem("@local", Action::Custom(MMAction::ReloadNext(Some(0))));
    def_sem("@reloadnext", Action::Custom(MMAction::ReloadNext(None)));
    def_sem("@reloadprev", Action::Custom(MMAction::ReloadPrev));
    def_sem("@cycle", Action::Custom(MMAction::ReloadNext(None)));

    config.binds.check_cycles().map_err(anyhow::Error::msg)?;
    config.binds.retain(|_, actions| !actions.is_empty());
    config.binds.resolve_semantics();

    for actions in config.binds.values() {
        for a in actions {
            if let Action::Custom(mm) = &a {
                mm.validate()?;
            }
        }
    }

    debug!("Config computed: {config:?}");

    Ok(config)
}

/// Spawns a tokio task mapping f to reader segments.
/// Read aborts on error. Read errors are logged.
pub fn map_reader<E: SSS + std::fmt::Display>(
    reader: impl Read + SSS,
    f: impl FnMut(String) -> Result<(), E> + SSS,
    input_separator: Option<char>,
    abort_empty: Option<RenderSender<MMAction>>,
) -> tokio::task::JoinHandle<Result<usize, MapReaderError<E>>> {
    tokio::task::spawn_blocking(move || {
        let ret = if let Some(delim) = input_separator {
            map_chunks::<E>(read_to_chunks(reader, delim), f, true)
        } else {
            map_reader_lines::<E>(reader, f, true)
        }
        .elog();

        if let Some(render_tx) = abort_empty
            && matches!(ret, Ok(0))
        {
            let _ = render_tx.send(matchmaker::message::RenderCommand::NoMatch);
        }
        log::trace!("All items pushed");
        ret
    })
}

pub static COMMAND_ARGS: Mutex<Vec<std::ffi::OsString>> = Mutex::new(Vec::new());

fn parse_border_type(s: &str) -> ratatui::widgets::BorderType {
    match s.trim().to_ascii_lowercase().as_str() {
        "plain" | "thin" => ratatui::widgets::BorderType::Plain,
        "rounded" => ratatui::widgets::BorderType::Rounded,
        "double" => ratatui::widgets::BorderType::Double,
        _ => ratatui::widgets::BorderType::Thick,
    }
}

fn parse_blink_rate(s: &str) -> BlinkRate {
    match s.trim().to_ascii_lowercase().as_str() {
        "slow" => BlinkRate::Slow,
        "rapid" | "fast" => BlinkRate::Rapid,
        _ => BlinkRate::Normal,
    }
}

fn apply_nav_props(props: &[String], config: &mut Config) {
    config.render.ui.nav_mode = true;
    config.render.ui.nav_bar = None;
    config.render.action.border.sides = Some(ratatui::widgets::Borders::NONE);
    config.render.query.status_inline = true;

    for raw in props {
        for prop in raw.split(',').filter(|s| !s.is_empty()) {
            match prop.split_once(':') {
                None => match prop {
                    "bar" => {
                        config.render.ui.nav_bar = Some(ratatui::widgets::BorderType::Thick);
                    }
                    "action-bar" => {
                        config.render.action.border.sides = Some(ratatui::widgets::Borders::BOTTOM);
                    }
                    "blink" => config.render.ui.nav_blink = true,
                    "bold" => config.render.ui.nav_bold = true,
                    "notify" => config.render.ui.nav_notify = true,
                    "passthrough" => config.render.ui.nav_passthrough = true,
                    "no-filter" => {
                        config.render.query.show = false;
                    }
                    "basic" => {
                        config.render.ui.nav_basic = true;
                        // Silence only the directory-navigation binds (h/l).
                        // Position-jump binds (gg, G, gb, gt) are kept so
                        // basic mode still supports full vertical navigation.
                        let empty = matchmaker::action::Actions::default();
                        for key in &["h", "l"] {
                            config
                                .render
                                .ui
                                .nav_binds
                                .insert(key.to_string(), empty.clone());
                        }
                    }
                    "hints" => config.render.ui.nav_hints = true,
                    "no-hints" => config.render.ui.nav_hints = false,
                    "parent-peek" | "parent_peek" => config.render.ui.parent_peek.enabled = true,
                    "no-parent-peek" | "no_parent_peek" => {
                        config.render.ui.parent_peek.enabled = false
                    }
                    "status-inline" => config.render.query.status_inline = true,
                    "no-status-inline" => config.render.query.status_inline = false,
                    _ => eprintln!("warning: unknown --nav property '{}'", prop),
                },
                Some(("bar", s)) => {
                    config.render.ui.nav_bar = Some(parse_border_type(s));
                }
                Some(("action-bar", s)) => {
                    config.render.action.border.sides = Some(ratatui::widgets::Borders::BOTTOM);
                    config.render.action.border.r#type = Some(parse_border_type(s));
                }
                Some(("blink", s)) => {
                    config.render.ui.nav_blink = true;
                    config.render.ui.nav_blink_rate = parse_blink_rate(s);
                }
                Some(("hints", s)) => match s.trim().to_ascii_lowercase().as_str() {
                    "false" | "off" | "no" | "0" => config.render.ui.nav_hints = false,
                    _ => config.render.ui.nav_hints = true,
                },
                Some(("parent-peek" | "parent_peek", s)) => {
                    match s.trim().to_ascii_lowercase().as_str() {
                        "false" | "off" | "no" | "0" => {
                            config.render.ui.parent_peek.enabled = false
                        }
                        _ => config.render.ui.parent_peek.enabled = true,
                    }
                }
                Some(("parent-peek-pct" | "parent_peek_pct", s)) => {
                    if let Ok(pct) = s.parse::<u16>() {
                        config.render.ui.parent_peek.pct = matchmaker::config::Percentage::new(pct);
                    }
                }
                Some(("parent-peek-color" | "parent_peek_color", s)) => {
                    if let Ok(color) = s.trim().parse::<ratatui::style::Color>() {
                        config.render.ui.parent_peek.parent_color = Some(color);
                    }
                }
                Some(("parent-peek-highlight" | "parent_peek_highlight", s)) => {
                    match s.trim().to_ascii_lowercase().as_str() {
                        "false" | "off" | "no" | "0" => {
                            config.render.ui.parent_peek.highlight = false
                        }
                        _ => config.render.ui.parent_peek.highlight = true,
                    }
                }
                Some(("parent-peek-highlight-color" | "parent_peek_highlight_color", s)) => {
                    if let Ok(color) = s.trim().parse::<ratatui::style::Color>() {
                        config.render.ui.parent_peek.highlight_color = Some(color);
                    }
                }
                Some(("parent-peek-border" | "parent_peek_border", s)) => {
                    match s.trim().to_ascii_lowercase().as_str() {
                        "false" | "off" | "no" | "0" | "none" => {
                            config.render.ui.parent_peek.border.show = false
                        }
                        _ => config.render.ui.parent_peek.border.show = true,
                    }
                }
                Some(("parent-peek-border-color" | "parent_peek_border_color", s)) => {
                    if let Ok(color) = s.trim().parse::<ratatui::style::Color>() {
                        config.render.ui.parent_peek.border.color = Some(color);
                    }
                }
                Some(("parent-peek-border-type" | "parent_peek_border_type", s)) => {
                    config.render.ui.parent_peek.border.r#type = Some(parse_border_type(s));
                }
                Some(("focus-on-start", s)) => match s.trim().to_ascii_lowercase().as_str() {
                    "picker" => {
                        config.render.ui.nav_focus_on_start = matchmaker::config::NavFocus::Picker
                    }
                    _ => config.render.ui.nav_focus_on_start = matchmaker::config::NavFocus::Filter,
                },
                Some(("marker", s)) => config.render.ui.nav_marker = s.to_string(),
                Some(("prompt", s)) => config.render.ui.nav_prompt = s.to_string(),
                Some(("color", s)) => match s.trim().parse::<ratatui::style::Color>() {
                    Ok(color) => config.render.ui.nav_color = color,
                    Err(e) => eprintln!("warning: invalid --nav color '{}': {}", s, e),
                },
                Some((k, _)) => eprintln!("warning: unknown --nav property '{}'", k),
            }
        }
    }
}

fn apply_media_props(props: &[String], config: &mut Config) {
    config.render.preview.media = true;

    for raw in props {
        for prop in raw.split(',').filter(|s| !s.is_empty()) {
            match prop.split_once(':') {
                None => {
                    // Try parsing as a standalone protocol or size
                    match prop.to_ascii_lowercase().as_str() {
                        "kitty" | "sixel" | "halfblocks" | "iterm2" => {
                            config.render.preview.media_protocol = Some(prop.to_string());
                        }
                        "xs" => config.previewer.media_size = 128,
                        "s" => config.previewer.media_size = 256,
                        "m" => config.previewer.media_size = 512,
                        "l" => config.previewer.media_size = 1024,
                        "xl" => config.previewer.media_size = 2048,
                        "full" => config.previewer.media_size = 0,
                        _ => {
                            if let Ok(num) = prop.parse::<u32>() {
                                config.previewer.media_size = num;
                            } else {
                                eprintln!("warning: unknown --media property '{}'", prop);
                            }
                        }
                    }
                }
                Some(("size", s)) => match s.to_ascii_lowercase().as_str() {
                    "xs" => config.previewer.media_size = 128,
                    "s" => config.previewer.media_size = 256,
                    "m" => config.previewer.media_size = 512,
                    "l" => config.previewer.media_size = 1024,
                    "xl" => config.previewer.media_size = 2048,
                    "full" => config.previewer.media_size = 0,
                    _ => {
                        if let Ok(num) = s.parse::<u32>() {
                            config.previewer.media_size = num;
                        } else {
                            eprintln!("warning: invalid --media size value '{}'", s);
                        }
                    }
                },
                Some(("type" | "protocol", s)) => {
                    config.render.preview.media_protocol = Some(s.to_string());
                }
                Some((k, _)) => eprintln!("warning: unknown --media property '{}'", k),
            }
        }
    }
}

/// Split a nav-bind action string on ';' while ignoring semicolons inside
/// parentheses. This allows `Execute(cd {};ls)` to stay a single action.
fn split_nav_bind_actions(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth: usize = 0;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }

    let tail = s[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }

    parts
}

pub fn process_envs(mut envs: HashMap<String, EnvValue>) -> HashMap<String, String> {
    let mut processed_envs = HashMap::new();

    // todo: lowpri: should we provision this what is the cost of setting more env vars
    if envs.get("CLIPcmd").is_none() {
        if let Some(v) = std::env::var("CLIPcmd").ok()
            && !v.is_empty()
        {
            envs.insert("CLIPcmd".to_string(), EnvValue::new(v));
        } else {
            if let Some((clip, paste)) = guess_clip_cmd() {
                envs.insert("CLIPcmd".to_string(), EnvValue::new(clip));

                if envs.get("PASTEcmd").is_none()
                    && std::env::var("PASTEcmd")
                        .ok()
                        .map_or(true, |x| x.is_empty())
                {
                    envs.insert("PASTEcmd".to_string(), EnvValue::new(paste));
                }
            }
        }
    }

    if envs.get("PAGER").is_none() && std::env::var("PAGER").ok().map_or(true, |x| x.is_empty()) {
        let ev = EnvValue::new(guess_pager_cmd());
        envs.insert("PAGER".to_string(), ev);
    }

    if envs.get("EDITOR").is_none() && std::env::var("EDITOR").ok().map_or(true, |x| x.is_empty()) {
        let ev = EnvValue::new(guess_editor_cmd());
        envs.insert("PAGER".to_string(), ev);
    }

    // First pass: static envs
    for (k, v) in &envs {
        if !v.value.is_empty() && !v.exec {
            if v.force || std::env::var_os(k).is_none() {
                processed_envs.insert(k.clone(), v.value.to_string());
            }
        }
    }

    // Second pass: dynamic envs
    for (k, v) in &envs {
        if !v.value.is_empty() && v.exec {
            if v.force || std::env::var_os(k).is_none() {
                if let Some(output) = Command::from_script(&v.value)
                    .envs(&processed_envs)
                    .read_to_string()
                    ._elog()
                {
                    processed_envs.insert(k.clone(), output.trim().to_string());
                } else {
                    _wbog!("Failed to execute env command for {}: {}", k, v.value);
                }
            }
        }
    }

    processed_envs
}

pub async fn start(
    config: Config,
    no_read: bool,
    group_prefix: Option<String>,
) -> Result<(), MatchError> {
    let nav_mode = config.render.ui.nav_mode;
    let nav_notify = config.render.ui.nav_notify;

    let Config {
        render,
        tui,
        previewer,
        matcher: MatcherConfig {
            matcher,
            mut worker,
        },
        columns,
        binds,
        start:
            StartConfig {
                input_separator,
                command:
                    CommandSetting {
                        separator,
                        command,
                        base_command,
                    },
                directory,
                sync,
                output_separator,
                output_template,
                ansi,
                trim,
                mut additional_commands,
                mode,
                sort,
                reload_interval,
            },
        mut exit,
        mut envs,
        source: _,
        rule: all_rules,
        ..
    } = config;

    let default_base_cmd = base_command
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| command.clone());

    if sort && !worker.sort_threshold.is_smart() {
        // Force nucleo to preserve insertion order (stable sort) so the alphabetically
        // sorted input is displayed in the same order when no query is typed.
        worker.sort_threshold = matchmaker::config::SortThreshold::NEVER;
    }

    // -------- determine command ------------
    if let Some(first) = additional_commands.first_mut() {
        if first.is_empty() {
            *first = command.clone();
        }
    }
    let additional_commands = additional_commands;

    let mut initial_index = 0;
    if additional_commands.len() > 1 {
        if let Ok(index_str) = std::env::var("MM_INDEX") {
            if let Ok(index) = index_str.parse::<usize>() {
                if index < additional_commands.len() {
                    initial_index = index;
                }
            }
        }
    }

    let command = if initial_index > 0 {
        additional_commands[initial_index].clone()
    } else {
        command
    };

    let initial_cmd = (!command.is_empty() && std::io::stdin().is_terminal() || no_read)
        .then_some(command.clone())
        .unwrap_or_default();

    // -------- set envs/directory -----------
    if !additional_commands.is_empty() {
        envs.insert(
            "MM_INDEX".to_string(),
            EnvValue::new(initial_index.to_string()),
        );
    }
    let envs = process_envs(envs);

    if let Ok(cwd) = std::env::current_dir() {
        let store = matchmaker::frecency::FrecencyStore::open();
        store.auto_import_from_zoxide_if_empty();
        let _ = store.add(&cwd.to_string_lossy());
    }

    if !directory.value.is_empty() {
        let EnvValue { value, force, exec } = directory;

        let mut failed = false;
        if exec {
            if let Some(new_d) = Command::from_script(&value)
                .envs(&envs)
                .read_to_string()
                ._elog()
            {
                let new_d = Path::new(new_d.trim()).to_path_buf();
                if new_d.exists() {
                    failed = set_current_dir(&new_d)
                        .prefix(format!("Failed to switch to {new_d:?}"))
                        ._wbog()
                        .is_some();
                } else {
                    ebog!("Directory does not exist: {}", new_d.display());
                    failed = true;
                }
            } else {
                ebog!("Failed to execute script for directory: {}", value);
                failed = true;
            }
        } else {
            let path = expand_tilde(value.into());
            set_current_dir(&path)
                .prefix(format!("Failed to switch to {path:?}"))
                ._wbog();
        }

        if failed && force {
            std::process::exit(1);
        }
    }

    // ---------------------------------

    let abort_empty = exit.abort_empty;
    let header_lines = render.header.header_lines;
    let print_handle = AppendOnly::new();
    let output_separator = output_separator.clone().unwrap_or("\n".into());
    let preprocess = (ansi, trim);

    if exit.last_key_path.is_none() {
        exit.last_key_path = Some(last_key_path().into())
    }

    let event_loop = EventLoop::with_binds(binds).with_tick_rate(render.tick_rate());

    // set event loop mode
    let mode = if let Some(m) = mode {
        m
    } else {
        match (
            !initial_cmd.is_empty(), // has command => t0
            std::io::stdout().is_terminal(),
        ) {
            (true, true) => "tty",
            (true, false) => "t0",
            (false, true) => "piped",
            (false, false) => "t1",
        }
        .to_string()
    };
    log::trace!("mode: {}", mode);
    if let Ok(mut m) = matchmaker::MODE.lock() {
        *m = mode;
    }

    // make matcher and matchmaker with matchmaker-and-matcher-maker
    let copy_trailing_newline = tui.copy_trailing_newline;
    let (
        mut mm,
        injector,
        OddEnds {
            splitter,
            hidden_columns,
            has_error,
        },
    ) = Matchmaker::new_from_config(render, tui, worker, columns, exit, preprocess);

    if has_error {
        return Err(MatchError::Abort(1));
    }
    // make previewer

    if !event_loop.binds.check_traces() {
        // maybe abort with error
    }
    let cli_formatter = Either::Right(
        crate::formatter::format_cli
            as for<'a, 'b, 'c> fn(
                &'a MMState<'b, 'c, matchmaker::ConfigMMItem, matchmaker::ConfigMMInnerItem>,
                &'a str,
                Option<&dyn Fn(String)>,
            ) -> String,
    );
    let binds = event_loop.binds.clone();
    let previewer = make_previewer(
        &mut mm,
        previewer,
        cli_formatter.clone(),
        Box::new(move |config, mode| matchmaker::binds::display_help(&binds, config, Some(mode))),
    );

    // ---------------------- build options ---------------------------

    let bind_tx = event_loop.bind_controller();

    let envs_ = envs.clone();
    let mut options = PickOptions::new()
        .event_loop(event_loop)
        .matcher(matcher.0)
        .previewer(previewer)
        .hidden_columns(hidden_columns)
        .initializer(move |s| {
            s.envs.extend(envs_);
        });

    let render_tx = options.render_tx();
    let push_fn = inject_line(
        header_lines,
        render_tx.clone(),
        injector,
        group_prefix.clone(),
    );

    if let Some(interval) = reload_interval {
        let render_tx = render_tx.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(interval));
            ticker.tick().await; // skip first immediate tick
            loop {
                ticker.tick().await;
                if render_tx
                    .send(matchmaker::message::RenderCommand::Action(
                        matchmaker::action::Action::Reload("".to_string()),
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    // ---------------------- register handlers ---------------------------
    // print handler (no quoting)
    mm._register_print_handler(
        print_handle.clone(),
        output_separator.clone(),
        cli_formatter.clone(),
    );

    // execute handlers
    mm.register_execute_handler(cli_formatter.clone());
    mm._register_execute_async_handler(cli_formatter.clone());
    mm.register_copy(
        cli_formatter.clone(),
        copy_trailing_newline,
        Some(render_tx.clone()),
    );
    mm._register_become_handler(cli_formatter.clone());
    // Speculative directory cache & background scanning
    let speculative_cache: Arc<Mutex<HashMap<std::path::PathBuf, Vec<String>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let spec_cache_chdir = speculative_cache.clone();
    let spec_cache_cursor = speculative_cache.clone();
    let spec_cache_reload = speculative_cache.clone();

    let chdir_formatter = cli_formatter.clone();
    let chdir_render_tx = render_tx.clone();
    let mut history: std::collections::HashMap<std::path::PathBuf, String> =
        std::collections::HashMap::new();
    mm.register_interrupt_handler(Interrupt::ChDir, move |state| {
        let template = state.payload().clone();
        if template.is_empty() {
            return;
        }
        let mut path = use_formatter(&chdir_formatter, state, &template, None);
        if path.is_empty() {
            return;
        }

        if path.starts_with("~/") || path == "~" {
            if let Ok(home) = std::env::var("HOME") {
                if path == "~" {
                    path = home;
                } else {
                    path = format!("{home}/{}", &path[2..]);
                }
            }
        }

        let target_path = Path::new(&path);
        let target_dir = if target_path.is_file() {
            target_path.parent().unwrap_or(target_path).to_path_buf()
        } else if target_path.is_relative() {
            let current_dir = std::env::current_dir().unwrap_or_default();
            let p = current_dir.join(target_path);
            if p.exists() {
                p
            } else {
                target_path.to_path_buf()
            }
        } else {
            target_path.to_path_buf()
        };

        let mut target_to_select = None;
        if target_dir == Path::new("..") {
            if let Ok(cwd) = std::env::current_dir() {
                if let Some(name) = cwd.file_name() {
                    target_to_select = Some(name.to_string_lossy().to_string());
                }
            }
        }

        let mut old_cwd = None;
        if state.ui.config.nav_mode {
            if let Ok(cwd) = std::env::current_dir() {
                history.insert(cwd.clone(), state.picker_ui.query.input.clone());

                // Save old_cwd items in speculative cache so returning to parent is instant
                let items: Vec<String> = state
                    .picker_ui
                    .worker
                    .raw_results()
                    .map(|item| state.picker_ui.worker.columns[0].raw(item).into_owned())
                    .collect();
                if !items.is_empty() {
                    if let Ok(mut c) = spec_cache_chdir.lock() {
                        if c.len() >= 64 {
                            c.clear();
                        }
                        c.insert(cwd.clone(), items);
                    }
                }

                old_cwd = Some(cwd);
            }
        }

        log::debug!("ChDir: {}", target_dir.display());
        if let Err(e) = std::env::set_current_dir(&target_dir) {
            log::warn!("ChDir({}) failed: {e}", target_dir.display());
        } else {
            if let Some(t) = target_to_select {
                unsafe {
                    std::env::set_var("MM_TARGET_ITEM", t);
                }
            } else {
                unsafe {
                    std::env::remove_var("MM_TARGET_ITEM");
                }
            }

            if let Ok(new_cwd) = std::env::current_dir() {
                let store = matchmaker::frecency::FrecencyStore::open();
                let _ = store.add(&new_cwd.to_string_lossy());
                if state.ui.config.nav_mode {
                    // Reset view to local mode (index 0) and nav mode upon entering/changing directory
                    let _ = chdir_render_tx.send(matchmaker::message::RenderCommand::Action(
                        matchmaker::action::Action::Custom(crate::action::MMAction::ReloadNext(Some(0))),
                    ));
                    let _ = chdir_render_tx.send(matchmaker::message::RenderCommand::Action(
                        matchmaker::action::Action::FocusNav,
                    ));
                    let is_parent = old_cwd
                        .as_ref()
                        .map_or(false, |old| old.starts_with(&new_cwd) && old != &new_cwd);
                    if is_parent {
                        if let Some(saved) = history.remove(&new_cwd) {
                            state.picker_ui.query.set(Some(saved), 0);
                        } else {
                            state.picker_ui.query.set(Some(String::new()), 0);
                        }
                    } else {
                        state.picker_ui.query.set(Some(String::new()), 0);
                    }
                }
            }
        }
    });

    let sync_formatter = cli_formatter.clone();
    mm.register_event_handler(Event::Synced, move |state, _| {
        if let Ok(target) = std::env::var("MM_TARGET_ITEM") {
            let count = state.picker_ui.worker.counts().0;
            let mut found = false;
            for i in 0..count {
                state.picker_ui.results.cursor_jump(i);
                let val = use_formatter(&sync_formatter, state, "{=}", None);
                let val_trimmed = val.trim_end_matches('/');
                let target_trimmed = target.trim_end_matches('/');
                if val_trimmed == target_trimmed {
                    found = true;
                    break;
                }
            }
            if !found && count > 0 {
                state.picker_ui.results.cursor_jump(0);
            }
            unsafe {
                std::env::remove_var("MM_TARGET_ITEM");
            }
        }
    });

    let reload_rules_spec = all_rules.clone();
    let default_base_cmd_spec = default_base_cmd;
    let get_active_cmd = move |dir: &std::path::Path| -> String {
        let mut active_cmd = default_base_cmd_spec.to_string();
        for rule in &reload_rules_spec {
            if rule.path.matches(dir) {
                if let Some(ref p) = rule.preset {
                    let mut preset_path = p.clone();
                    if preset_path.is_relative() && preset_path.extension().is_none() {
                        let main_p = presets_path().join(&preset_path).join("main.toml");
                        preset_path = if !main_p.exists() {
                            presets_path().join(preset_path.with_extension("toml"))
                        } else {
                            main_p
                        };
                    }
                    if let Ok(preset_config) =
                        load_type::<PartialConfig, _>(&preset_path, |s| toml::from_str(s))
                    {
                        if let Some(ref cmd_setting) = preset_config.start.command.as_ref() {
                            if !cmd_setting.command.is_empty() {
                                active_cmd = cmd_setting.command.clone();
                            }
                        }
                    }
                }
                if let Some(ref cmd_setting) = rule.override_config.start.command {
                    if !cmd_setting.command.is_empty() {
                        active_cmd = cmd_setting.command.clone();
                    }
                }
            }
        }
        active_cmd
    };

    let cursor_formatter = cli_formatter.clone();
    let spec_get_cmd = get_active_cmd.clone();
    let spec_sep = separator.or(input_separator).unwrap_or('\n');
    mm.register_event_handler(Event::CursorChange, move |state, _| {
        let val = use_formatter(&cursor_formatter, state, "{=}", None);
        if val.is_empty() {
            return;
        }
        let current_dir = std::env::current_dir().unwrap_or_default();
        let target_path = current_dir.join(val.trim_end_matches('/'));
        if target_path.is_dir() {
            let is_cached = spec_cache_cursor
                .lock()
                .map(|c| c.contains_key(&target_path))
                .unwrap_or(false);
            if !is_cached {
                let cache_ref = spec_cache_cursor.clone();
                let cmd_to_run = spec_get_cmd(&target_path);
                let target_dir = target_path.clone();
                let env_vars = state.make_env_vars();
                tokio::task::spawn_blocking(move || {
                    let lines: Option<Vec<String>> = if is_default_file_walker_command(&cmd_to_run)
                    {
                        let walker = matchmaker::walker::AsyncWalker::from_root(&target_dir);
                        Some(walker.collect_sync())
                    } else if let Some(out) = Command::from_script(&cmd_to_run)
                        .current_dir(&target_dir)
                        .envs(env_vars)
                        .stdin(Stdio::null())
                        .args(&*COMMAND_ARGS.lock().unwrap())
                        .output()
                        ._elog()
                    {
                        let text = String::from_utf8_lossy(&out.stdout);
                        let mut l: Vec<String> =
                            text.split(spec_sep).map(|s| s.to_string()).collect();
                        if l.last().map_or(false, |line| line.is_empty()) {
                            l.pop();
                        }
                        Some(l)
                    } else {
                        None
                    };

                    if let Some(lines) = lines {
                        if let Ok(mut c) = cache_ref.lock() {
                            if c.len() >= 64 {
                                c.clear();
                            }
                            c.insert(target_dir, lines);
                        }
                    }
                });
            }
        }
    });

    // reload handler
    let reload_formatter = cli_formatter.clone();
    let reload_render_tx = render_tx.clone();

    mm.register_interrupt_handler(Interrupt::Reload, move |state| {
        let current_dir = std::env::current_dir().unwrap_or_default();

        if let Ok(mut cache) = spec_cache_reload.lock() {
            if let Some(lines) = cache.remove(&current_dir) {
                debug!("Speculative Cache HIT for {current_dir:?}");
                state.picker_ui.worker.restart(false);
                state.reloading = true;

                let injector = state.injector();
                let injector = IndexedInjector::new_globally_indexed(injector);
                let injector = SegmentedInjector::new(injector, splitter.clone());
                let injector = AnsiInjector::new(injector, preprocess.clone());

                let mut push_fn = inject_line(
                    state.picker_ui.header.config.header_lines,
                    reload_render_tx.clone(),
                    injector,
                    group_prefix.clone(),
                );

                state.picker_ui.selector.clear();
                for line in lines {
                    let _ = push_fn(line);
                }

                let _ = reload_render_tx.send(matchmaker::message::RenderCommand::Action(
                    matchmaker::action::Action::Custom(crate::action::MMAction::ReloadReady(
                        vec![],
                    )),
                ));
                return;
            }
        }

        let cmd = if !state.payload().is_empty() {
            use_formatter(&reload_formatter, state, state.payload(), None)
        } else {
            get_active_cmd(&current_dir)
        };

        let is_bookmarks = cmd.contains("--bookmarks") || cmd.contains("--pins");
        let is_dirs = !is_bookmarks && (cmd.contains("--dirs") || cmd.starts_with("mm list -d"));

        if is_bookmarks {
            state.picker_ui.worker.restart(false);
            state.reloading = true;

            let injector = state.injector();
            let injector = IndexedInjector::new_globally_indexed(injector);
            let injector = SegmentedInjector::new(injector, splitter.clone());
            let injector = AnsiInjector::new(injector, preprocess.clone());

            let mut push_fn = inject_line(
                state.picker_ui.header.config.header_lines,
                reload_render_tx.clone(),
                injector,
                group_prefix.clone(),
            );

            state.picker_ui.selector.clear();
            let store = matchmaker::frecency::FrecencyStore::open();
            let pins = store.list_pins();
            for pin in pins {
                let _ = push_fn(pin);
            }

            let _ = reload_render_tx.send(matchmaker::message::RenderCommand::Action(
                matchmaker::action::Action::Custom(crate::action::MMAction::ReloadReady(
                    vec![],
                )),
            ));
        } else if is_dirs {
            state.picker_ui.worker.restart(false);
            state.reloading = true;

            let injector = state.injector();
            let injector = IndexedInjector::new_globally_indexed(injector);
            let injector = SegmentedInjector::new(injector, splitter.clone());
            let injector = AnsiInjector::new(injector, preprocess.clone());

            let mut push_fn = inject_line(
                state.picker_ui.header.config.header_lines,
                reload_render_tx.clone(),
                injector,
                group_prefix.clone(),
            );

            state.picker_ui.selector.clear();
            let store = matchmaker::frecency::FrecencyStore::open();
            store.auto_import_from_zoxide_if_empty();
            if let Ok(cwd) = std::env::current_dir() {
                let _ = store.add(&cwd.to_string_lossy());
            }
            let snapshot = store.get_snapshot_with_half_life(30);
            let mut items: Vec<(String, u32)> = snapshot.scores.into_iter().collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));
            for (path, _) in items {
                let _ = push_fn(path);
            }

            let _ = reload_render_tx.send(matchmaker::message::RenderCommand::Action(
                matchmaker::action::Action::Custom(crate::action::MMAction::ReloadReady(
                    vec![],
                )),
            ));
        } else if is_default_file_walker_command(&cmd) {
            let cwd_str = current_dir.to_string_lossy().to_string();
            let cache_store = matchmaker::cache::DirCacheStore::open();

            if let Some(cached_rec) = cache_store.get_valid(&cwd_str)
                && !cached_rec.items.is_empty()
            {
                debug!("DirCacheStore HIT for {cwd_str}");
                state.picker_ui.worker.restart(false);
                state.reloading = true;

                let injector = state.injector();
                let injector = IndexedInjector::new_globally_indexed(injector);
                let injector = SegmentedInjector::new(injector, splitter.clone());
                let injector = AnsiInjector::new(injector, preprocess.clone());

                let mut push_fn = inject_line(
                    state.picker_ui.header.config.header_lines,
                    reload_render_tx.clone(),
                    injector,
                    group_prefix.clone(),
                );

                state.picker_ui.selector.clear();
                for item in cached_rec.items {
                    let _ = push_fn(item);
                }

                let _ = reload_render_tx.send(matchmaker::message::RenderCommand::Action(
                    matchmaker::action::Action::Custom(crate::action::MMAction::ReloadReady(
                        vec![],
                    )),
                ));

                // Background async walk to refresh disk cache if needed
                let cwd_clone = cwd_str.clone();
                tokio::spawn(async move {
                    let (collect_tx, collect_rx) = std::sync::mpsc::channel();
                    let walker = matchmaker::walker::AsyncWalker::from_root(&cwd_clone);
                    let handle = walker.spawn_walk(move |line| {
                        let _ = collect_tx.send(line);
                        Ok(())
                    });
                    let _ = handle.await;

                    let mut fresh_items: Vec<String> = collect_rx.into_iter().collect();
                    if !fresh_items.is_empty() {
                        fresh_items.sort_by_key(|item| {
                            let slashes = item.bytes().filter(|&b| b == b'/' || b == b'\\').count();
                            (slashes, item.clone())
                        });
                        let cache_store = matchmaker::cache::DirCacheStore::open();
                        let _ = cache_store.put(&cwd_clone, fresh_items);
                    }
                });
                return;
            }

            state.picker_ui.worker.restart(false);
            state.reloading = true;

            if let Ok(mut c) = spec_cache_reload.lock() {
                c.clear();
            }

            let injector = state.injector();
            let injector = IndexedInjector::new_globally_indexed(injector);
            let injector = SegmentedInjector::new(injector, splitter.clone());
            let injector = AnsiInjector::new(injector, preprocess.clone());

            let mut push_fn = inject_line(
                state.picker_ui.header.config.header_lines,
                reload_render_tx.clone(),
                injector,
                group_prefix.clone(),
            );

            state.picker_ui.selector.clear();
            let reload_render_tx = reload_render_tx.clone();
            let reload_render_tx_ready = reload_render_tx.clone();
            tokio::task::spawn_blocking(move || {
                let (collect_tx, collect_rx) = std::sync::mpsc::channel();
                let walker = matchmaker::walker::AsyncWalker::from_root(".");
                let mut first = true;
                let handle = walker.spawn_walk(move |line| {
                    let _ = collect_tx.send(line.clone());
                    let res = push_fn(line);
                    if first {
                        first = false;
                        let _ = reload_render_tx_ready.send(
                            matchmaker::message::RenderCommand::Action(
                                matchmaker::action::Action::Custom(
                                    crate::action::MMAction::ReloadReady(vec![]),
                                ),
                            ),
                        );
                    }
                    res
                });
                let _ = tokio::runtime::Handle::current().block_on(handle);

                let mut fresh_items: Vec<String> = collect_rx.into_iter().collect();
                if !fresh_items.is_empty() {
                    fresh_items.sort_by_key(|item| {
                        let slashes = item.bytes().filter(|&b| b == b'/' || b == b'\\').count();
                        (slashes, item.clone())
                    });
                    let cwd_str = std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let cache_store = matchmaker::cache::DirCacheStore::open();
                    let _ = cache_store.put(&cwd_str, fresh_items);
                }

                if first {
                    let _ = reload_render_tx.send(matchmaker::message::RenderCommand::Action(
                        matchmaker::action::Action::Custom(crate::action::MMAction::ReloadReady(
                            vec![],
                        )),
                    ));
                }
            });
        } else if !cmd.is_empty() {
            state.picker_ui.worker.restart(false);
            state.reloading = true;

            let injector = state.injector();
            let injector = IndexedInjector::new_globally_indexed(injector);
            let injector = SegmentedInjector::new(injector, splitter.clone());
            let injector = AnsiInjector::new(injector, preprocess.clone());

            let push_fn = inject_line(
                state.picker_ui.header.config.header_lines,
                reload_render_tx.clone(),
                injector,
                group_prefix.clone(),
            );

            let vars = state.make_env_vars();
            debug!("Reloading: {cmd} in {current_dir:?}");
            state.picker_ui.selector.clear();

            let separator = separator.or(input_separator).unwrap_or('\n');
            let reload_render_tx = reload_render_tx.clone();
            let cmd = cmd.clone();
            let current_dir = current_dir.clone();
            tokio::task::spawn_blocking(move || {
                if let Some(out) = Command::from_script(&cmd)
                    .current_dir(&current_dir)
                    .envs(vars)
                    .stdin(Stdio::null())
                    .args(&*COMMAND_ARGS.lock().unwrap())
                    .output()
                    ._elog()
                {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut lines: Vec<&str> = text.split(separator).collect();
                    if lines.last() == Some(&"") {
                        lines.pop();
                    }
                    let mut push_fn = push_fn;
                    for line in lines {
                        let _ = push_fn(line.to_string());
                    }
                }

                let _ = reload_render_tx.send(matchmaker::message::RenderCommand::Action(
                    matchmaker::action::Action::Custom(crate::action::MMAction::ReloadReady(
                        vec![],
                    )),
                ));
            });
        }
    });

    debug!("{mm:?}");

    let mut action_context = ActionContext {
        bind_tx,
        render_tx: render_tx.clone(),
        additional_commands: (additional_commands, initial_index),
        output_template,
        print_handle: print_handle.clone(),
        output_separator: output_separator.clone(),
        clipboard: Arc::new(Mutex::new(None)),
        fm_notify: nav_notify,
        undo_stack: Arc::new(Mutex::new(Vec::new())),
        redo_stack: Arc::new(Mutex::new(Vec::new())),
        fm_action: None,
        query_history: std::collections::HashMap::new(),
    };

    options = options
        .ext_handler(move |x, y| action_handler(x, y, &mut action_context))
        .ext_aliaser(|a, _state| match a {
            Action::Accept => acs![MMAction::Accept],
            Action::Semantic(ref s) if s == "fm_create" => acs![MMAction::FmCreateStart],
            Action::Semantic(ref s) if s == "fm_delete" => acs![MMAction::FmDeleteStart],
            Action::Semantic(ref s) if s == "fm_rename" => acs![MMAction::FmRenameStart],
            Action::Semantic(ref s) if s == "fm_unzip" => acs![MMAction::FmUnzipStart],
            Action::Semantic(ref s) if s == "fm_zip" => acs![MMAction::FmZipStart],
            Action::Semantic(ref s) if s == "fm_yank" => acs![MMAction::FmYank],
            Action::Semantic(ref s) if s == "fm_unyank" => acs![MMAction::FmUnyank],
            Action::Semantic(ref s) if s == "fm_cut" => acs![MMAction::FmCut],
            Action::Semantic(ref s) if s == "fm_uncut" => acs![MMAction::FmUncut],
            Action::Semantic(ref s) if s == "fm_paste" => acs![MMAction::FmPaste],
            Action::Semantic(ref s) if s == "fm_undo" => acs![MMAction::FmUndo],
            Action::Semantic(ref s) if s == "fm_redo" => acs![MMAction::FmRedo],
            Action::Semantic(ref s) if s == "fm_dragdrop" => acs![MMAction::FmDragDrop],
            Action::Semantic(ref s)
                if s == "fm_pin" || s == "fm_bookmark" || s == "pin" || s == "bookmark" =>
            {
                acs![MMAction::FmTogglePin]
            }
            Action::Semantic(ref s)
                if s == "pins" || s == "bookmarks" || s == "reload_pins" || s == "reload_bookmarks" =>
            {
                acs![MMAction::ReloadNext(Some(2))]
            }
            Action::Semantic(ref s)
                if s == "dirs" || s == "frecency" || s == "reload_dirs" || s == "reload_frecency" =>
            {
                acs![MMAction::ReloadNext(Some(1))]
            }
            Action::Semantic(ref s) if s == "cycle" => acs![MMAction::ReloadNext(None)],
            Action::Semantic(ref s) if s == "reloadnext" => acs![MMAction::ReloadNext(None)],
            Action::Semantic(ref s) if s == "reloadprev" => acs![MMAction::ReloadPrev],
            Action::Semantic(ref s) if s == "reload_local" || s == "local" => acs![MMAction::ReloadNext(Some(0))],
            _ => acs![a],
        });

    if nav_mode {
        log::debug!("Navigation mode enabled");
    }

    // ----------- read -----------------------
    let handle = if sort {
        // Collect all input, sort alphabetically, then inject in sorted order.
        let sep = separator.or(input_separator).unwrap_or('\n');
        let raw: Vec<u8> = if !std::io::stdin().is_terminal() && !no_read {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).ok();
            buf
        } else if is_default_file_walker_command(&command) {
            let walker = matchmaker::walker::AsyncWalker::from_root(".");
            let items = walker.collect_sync();
            items.join("\n").into_bytes()
        } else if !command.is_empty() {
            Command::from_script(&command)
                .envs(envs)
                .args(&*COMMAND_ARGS.lock().unwrap())
                .output()
                .map(|o| o.stdout)
                .unwrap_or_default()
        } else {
            eprintln!("error: no input detected.");
            std::process::exit(99)
        };

        let text = String::from_utf8_lossy(&raw);
        let mut lines: Vec<&str> = text.split(sep).collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        lines.sort_unstable();
        let sorted = lines.join("\n");
        drop(lines);

        map_reader(
            std::io::Cursor::new(sorted),
            push_fn,
            None, // already newline-separated after join
            abort_empty.then_some(render_tx),
        )
    } else if !std::io::stdin().is_terminal() && !no_read {
        let stdin = std::io::stdin();
        map_reader(
            stdin,
            push_fn,
            input_separator,
            abort_empty.then_some(render_tx),
        )
    } else if is_default_file_walker_command(&command) {
        let cwd_str = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let cache_store = matchmaker::cache::DirCacheStore::open();

        if let Some(cached_rec) = cache_store.get_valid(&cwd_str)
            && !cached_rec.items.is_empty()
        {
            let mut push_fn = push_fn;
            for item in cached_rec.items {
                if push_fn(item).is_err() {
                    break;
                }
            }

            tokio::spawn(async move {
                let (collect_tx, collect_rx) = std::sync::mpsc::channel();
                let walker = matchmaker::walker::AsyncWalker::from_root(".");
                let handle = walker.spawn_walk(move |line| {
                    let _ = collect_tx.send(line);
                    Ok(())
                });
                let _ = handle.await;

                let mut fresh_items: Vec<String> = collect_rx.into_iter().collect();
                if !fresh_items.is_empty() {
                    fresh_items.sort_by_key(|item| {
                        let slashes = item.bytes().filter(|&b| b == b'/' || b == b'\\').count();
                        (slashes, item.clone())
                    });
                    let _ = cache_store.put(&cwd_str, fresh_items);
                }
                Ok(0)
            })
        } else {
            tokio::spawn(async move {
                let (collect_tx, collect_rx) = std::sync::mpsc::channel();
                let walker = matchmaker::walker::AsyncWalker::from_root(".");
                let mut push_fn = push_fn;

                let handle = walker.spawn_walk(move |line| {
                    let _ = collect_tx.send(line.clone());
                    push_fn(line)
                });
                let _ = handle.await;

                let mut fresh_items: Vec<String> = collect_rx.into_iter().collect();
                if !fresh_items.is_empty() {
                    fresh_items.sort_by_key(|item| {
                        let slashes = item.bytes().filter(|&b| b == b'/' || b == b'\\').count();
                        (slashes, item.clone())
                    });
                    let _ = cache_store.put(&cwd_str, fresh_items);
                }
                Ok(0)
            })
        }
    } else if !command.is_empty()
        && let Some((mut _child, stdout)) = Command::from_script(&command)
            .envs(envs)
            .args(&*COMMAND_ARGS.lock().unwrap())
            .spawn_piped()
            ._ebog()
    {
        map_reader(
            stdout,
            push_fn,
            separator.or(input_separator),
            abort_empty.then_some(render_tx),
        )
    } else {
        eprintln!("error: no input detected.");
        std::process::exit(99)
    };

    if sync {
        handle.await._wbog(); // warn the mapreader error (?)
    }

    let ret = mm.pick(options).await;

    print_handle.map_to_vec(|s| {
        log::trace!("{s}"); // this apparently helps with a race condition that erases output?
        print!("{}{}", s, output_separator);
    });

    log::trace!("Print complete");

    ret.map(|_| {})
}

use matchmaker::nucleo::{Line, Span};

fn inject_line(
    header_lines: usize,
    render_tx: RenderSender<MMAction>,
    injector: ConfigInjector,
    group_prefix: Option<String>,
) -> impl FnMut(String) -> Result<(), matchmaker::nucleo::WorkerError> + Send {
    let mut header_buf = Vec::with_capacity(header_lines);
    let mut remaining = header_lines;
    let injector = injector;
    let mut current_group: Option<std::sync::Arc<str>> = None;

    // For each row, take the first line of each segmented column, building a Vec<Vec<Line>>
    move |line: String| {
        if let Some(prefix) = &group_prefix {
            if line.starts_with(prefix) {
                current_group = Some(line.strip_prefix(prefix).unwrap().trim().into());
                return Ok(());
            }
        }

        if remaining > 0 {
            let item = injector.wrap((current_group.clone(), line)).unwrap();
            let item = injector.injector.wrap(item).unwrap();
            header_buf.push(item);
            remaining -= 1;

            if remaining == 0 {
                let rows: Vec<Vec<Line>> = header_buf
                    .drain(..)
                    .map(|seg| {
                        let row = (0..seg.len())
                            .map(move |i| {
                                let mut s = seg.get_text(i);
                                if s.lines.is_empty() {
                                    Line::default()
                                } else {
                                    to_static(s.lines.remove(0))
                                }
                            })
                            .collect();
                        trim_trailing_empty(row)
                    })
                    .collect();

                let _ = render_tx.send(matchmaker::message::RenderCommand::HeaderTable(rows));
            }

            Ok(())
        } else {
            injector.push((current_group.clone(), line))
        }
    }
}

fn trim_trailing_empty(mut row: Vec<Line>) -> Vec<Line> {
    while matches!(row.last(), Some(line) if line.iter().all(|x| x.content.is_empty())) {
        row.pop();
    }

    row
}

fn to_static(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| {
                Span::styled(
                    span.content.into_owned(), // force ownership
                    span.style,
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn is_default_file_walker_command(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    trimmed.is_empty()
        || (trimmed.contains("fd") && trimmed.contains("find"))
        || trimmed == "fd --strip-cwd-prefix --print0"
        || trimmed == "find . -print0"
}
