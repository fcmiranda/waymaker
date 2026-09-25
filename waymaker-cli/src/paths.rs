use std::path::PathBuf;

use cba::expr_as_path_fn;

use crate::clap::LIBRARY_FULL;

fn config_dir_impl() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        let config = home.join(".config").join(LIBRARY_FULL);
        if config.exists() {
            return Some(config);
        }
        let legacy_config = home.join(".config").join("matchmaker");
        if legacy_config.exists() {
            return Some(legacy_config);
        }
    };

    dirs::config_dir().map(|x| {
        let p = x.join(LIBRARY_FULL);
        if p.exists() {
            p
        } else {
            let legacy = x.join("matchmaker");
            if legacy.exists() {
                legacy
            } else {
                p
            }
        }
    })
}

pub fn state_dir_impl() -> Option<PathBuf> {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))?;
    let p = base.join(LIBRARY_FULL);
    if p.exists() {
        Some(p)
    } else {
        let legacy = base.join("matchmaker");
        if legacy.exists() {
            Some(legacy)
        } else {
            Some(p)
        }
    }
}

expr_as_path_fn!(state_dir, state_dir_impl().unwrap_or_default());
expr_as_path_fn!(
    last_key_path,
    state_dir_impl().unwrap_or_default().join("last_key")
);

#[cfg(debug_assertions)]
expr_as_path_fn!(
    default_config_path,
    config_dir_impl().unwrap_or_default().join("dev.toml")
);

expr_as_path_fn!(
    presets_path,
    default_config_path()
        .parent()
        .unwrap_or(&std::path::Path::new(""))
        .join("presets")
);

#[cfg(not(debug_assertions))]
expr_as_path_fn!(
    default_config_path,
    config_dir_impl().unwrap_or_default().join("config.toml")
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_resolution() {
        let state = state_dir();
        assert!(!state.as_os_str().is_empty());
        assert!(state.to_string_lossy().contains(LIBRARY_FULL) || state.to_string_lossy().contains("matchmaker"));

        let last_key = last_key_path();
        assert!(last_key.ends_with("last_key"));

        let presets = presets_path();
        assert!(presets.ends_with("presets"));

        let default_config = default_config_path();
        assert!(default_config.ends_with("dev.toml") || default_config.ends_with("config.toml"));

        if dirs::home_dir().is_some() {
            assert!(state_dir_impl().is_some());
            assert!(config_dir_impl().is_some());
        }
    }
}
