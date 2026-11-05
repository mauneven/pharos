//! XDG locations Beacon reads and writes.

use std::path::PathBuf;

pub const APP_ID: &str = "com.mauneven.beacon";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() && PathBuf::from(&v).is_absolute() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn config_home() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn state_home() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state")
}

pub fn config_file() -> PathBuf {
    config_home().join("beacon/config.toml")
}

pub fn autostart_file() -> PathBuf {
    config_home().join(format!("autostart/{APP_ID}.desktop"))
}

/// The directory holding `theme.generation` and the `theme/` folder. Newer
/// Omarchy keeps it under the state dir; older releases used `~/.config`.
pub fn omarchy_current() -> PathBuf {
    let state = state_home().join("omarchy/current");
    if state.join("theme").is_dir() {
        return state;
    }
    let legacy = config_home().join("omarchy/current");
    if legacy.join("theme").is_dir() {
        return legacy;
    }
    state
}

pub fn omarchy_user_shell_toml() -> PathBuf {
    config_home().join("omarchy/shell.toml")
}

pub fn fontconfig_file() -> PathBuf {
    config_home().join("fontconfig/fonts.conf")
}
