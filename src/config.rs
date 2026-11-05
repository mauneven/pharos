//! `~/.config/beacon/config.toml`: the master switch and the reminder list.
//!
//! A reminder Beacon created (`preset = "water"`) has no stored name, so it
//! shows in whatever language Omarchy uses. Once the user renames it, the
//! text is stored in `name` and never touched again. Minutes and the on/off
//! state are independent of the name.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;

use crate::{i18n, paths};

pub const MIN_MINUTES: u32 = 1;
pub const MAX_MINUTES: u32 = 120;

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reminder {
    pub id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub minutes: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Reminder {
    pub fn preset(id: u32, preset: &str, minutes: u32, enabled: bool) -> Self {
        Reminder {
            id,
            preset: Some(preset.to_string()),
            name: None,
            minutes,
            enabled,
        }
    }

    /// The user's own name, or the preset's name in the current language.
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.preset.as_deref().and_then(i18n::preset_name))
            .unwrap_or_default()
    }

    /// Text typed in the name field. Typing the preset's own translation
    /// back keeps the reminder localized; anything else is a custom name.
    pub fn rename(&mut self, text: &str) {
        let localized = self.preset.as_deref().and_then(i18n::preset_name);
        if localized.as_deref() == Some(text) {
            self.name = None;
        } else {
            self.name = Some(text.to_string());
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, rename = "reminder")]
    pub reminders: Vec<Reminder>,
}

impl Default for Config {
    fn default() -> Self {
        let reminders = i18n::PRESETS
            .iter()
            .enumerate()
            .map(|(i, (preset, minutes, enabled))| {
                Reminder::preset(i as u32 + 1, preset, *minutes, *enabled)
            })
            .collect();
        Config {
            enabled: true,
            reminders,
        }
    }
}

pub enum Loaded {
    /// No config existed; defaults were written.
    FirstRun(Config),
    Existing(Config),
}

impl Config {
    pub fn load() -> Loaded {
        let path = paths::config_file();
        match std::fs::read_to_string(&path) {
            Ok(raw) => match toml::from_str::<Config>(&raw) {
                Ok(mut cfg) => {
                    cfg.sanitize();
                    Loaded::Existing(cfg)
                }
                Err(err) => {
                    // Keep the broken file for the user instead of overwriting it.
                    let backup = path.with_extension("toml.broken");
                    let _ = std::fs::rename(&path, &backup);
                    eprintln!(
                        "beacon: {} is invalid ({err}); moved it to {} and started fresh",
                        path.display(),
                        backup.display()
                    );
                    let cfg = Config::default();
                    let _ = cfg.save();
                    Loaded::Existing(cfg)
                }
            },
            Err(_) => {
                let cfg = Config::default();
                if let Err(err) = cfg.save() {
                    eprintln!("beacon: could not write {}: {err}", path.display());
                }
                Loaded::FirstRun(cfg)
            }
        }
    }

    fn sanitize(&mut self) {
        let mut seen = HashSet::new();
        let mut next = self.reminders.iter().map(|r| r.id).max().unwrap_or(0);
        for r in &mut self.reminders {
            r.minutes = r.minutes.clamp(MIN_MINUTES, MAX_MINUTES);
            if r.id == 0 || !seen.insert(r.id) {
                next += 1;
                r.id = next;
                seen.insert(r.id);
            }
        }
    }

    /// Atomic write: a crash mid-save never leaves a half-written config.
    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::config_file();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(
                b"# Beacon reminders: every `minutes`, a notification, then the clock restarts.\n\
                  # `preset` reminders follow Omarchy's language until you give them a `name`.\n\n",
            )?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(tmp, path)
    }

    pub fn next_id(&self) -> u32 {
        self.reminders.iter().map(|r| r.id).max().unwrap_or(0) + 1
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Reminder> {
        self.reminders.iter_mut().find(|r| r.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(!text.contains("name ="), "presets must not store a name");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn rename_keeps_or_breaks_localization() {
        let mut r = Reminder::preset(1, "water", 20, true);
        let localized = r.display_name();
        r.rename(&localized);
        assert_eq!(r.name, None);
        r.rename("Agua!");
        assert_eq!(r.name.as_deref(), Some("Agua!"));
        assert_eq!(r.display_name(), "Agua!");
    }

    #[test]
    fn sanitize_fixes_ids_and_ranges() {
        let mut cfg: Config = toml::from_str(
            r#"
            [[reminder]]
            id = 1
            name = "a"
            minutes = 0
            [[reminder]]
            id = 1
            name = "b"
            minutes = 999
            "#,
        )
        .unwrap();
        cfg.sanitize();
        assert!(cfg.enabled);
        assert_eq!(cfg.reminders[0].minutes, MIN_MINUTES);
        assert_eq!(cfg.reminders[1].minutes, MAX_MINUTES);
        assert_ne!(cfg.reminders[0].id, cfg.reminders[1].id);
        assert!(cfg.reminders[1].enabled);
    }
}
