//! `~/.config/pharos/config.toml`: the master switch and the reminder list.
//!
//! A reminder Pharos created (`preset = "water"`) has no stored name, so it
//! shows in whatever language Omarchy uses. Once the user renames it, the
//! text is stored in `name` and never touched again. Minutes and the on/off
//! state are independent of the name.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

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
        Config::load_from(&paths::config_file())
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&paths::config_file())
    }

    /// Reads a config file. A missing file is a first run: defaults are
    /// written. A broken file is kept aside as `*.toml.broken`.
    pub fn load_from(path: &Path) -> Loaded {
        match std::fs::read_to_string(path) {
            Ok(raw) => match toml::from_str::<Config>(&raw) {
                Ok(mut cfg) => {
                    cfg.sanitize();
                    Loaded::Existing(cfg)
                }
                Err(err) => {
                    // Keep the broken file for the user instead of overwriting it.
                    let backup = path.with_extension("toml.broken");
                    let _ = std::fs::rename(path, &backup);
                    eprintln!(
                        "pharos: {} is invalid ({err}); moved it to {} and started fresh",
                        path.display(),
                        backup.display()
                    );
                    let cfg = Config::default();
                    let _ = cfg.save_to(path);
                    Loaded::Existing(cfg)
                }
            },
            Err(_) => {
                let cfg = Config::default();
                if let Err(err) = cfg.save_to(path) {
                    eprintln!("pharos: could not write {}: {err}", path.display());
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
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(
                b"# Pharos reminders: every `minutes`, a notification, then the clock restarts.\n\
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
        let _lang = crate::testutil::language("en");
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

    use crate::testutil::{TempDir, language};

    fn names(cfg: &Config) -> Vec<String> {
        cfg.reminders.iter().map(Reminder::display_name).collect()
    }

    #[test]
    fn first_run_writes_the_defaults() {
        let dir = TempDir::new("first-run");
        let path = dir.path().join("pharos/config.toml");
        let Loaded::FirstRun(cfg) = Config::load_from(&path) else {
            panic!("a missing file is a first run");
        };
        assert_eq!(cfg, Config::default());
        assert!(path.is_file(), "defaults are saved right away");
        let Loaded::Existing(again) = Config::load_from(&path) else {
            panic!("the second start is not a first run");
        };
        assert_eq!(again, cfg);
    }

    #[test]
    fn defaults_match_the_app() {
        let cfg = Config::default();
        assert!(cfg.enabled);
        let minutes: Vec<u32> = cfg.reminders.iter().map(|r| r.minutes).collect();
        assert_eq!(minutes, [20, 24, 40, 10, 30]);
        let on: Vec<bool> = cfg.reminders.iter().map(|r| r.enabled).collect();
        assert_eq!(on, [true, false, true, true, true]);
    }

    #[test]
    fn edits_survive_a_restart() {
        let dir = TempDir::new("edits");
        let path = dir.path().join("config.toml");
        let mut cfg = Config {
            enabled: false,
            ..Config::default()
        };
        cfg.reminders[0].minutes = 55;
        cfg.reminders[1].enabled = true;
        cfg.reminders[2].rename("Walk the dog");
        cfg.reminders.remove(3);
        let id = cfg.next_id();
        cfg.reminders
            .push(Reminder::preset(id, i18n::NEW_PRESET, 30, true));
        cfg.save_to(&path).unwrap();
        let Loaded::Existing(back) = Config::load_from(&path) else {
            panic!()
        };
        assert_eq!(back, cfg);
        assert!(
            !dir.path().join("config.toml.tmp").exists(),
            "no temp file left over"
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("# Pharos reminders")
        );
    }

    #[test]
    fn a_broken_file_is_kept_aside() {
        let dir = TempDir::new("broken");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is [not toml").unwrap();
        let Loaded::Existing(cfg) = Config::load_from(&path) else {
            panic!("a broken file is not a first run")
        };
        assert_eq!(cfg, Config::default());
        let kept = dir.path().join("config.toml.broken");
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "this is [not toml");
    }

    #[test]
    fn unedited_reminders_follow_the_language() {
        let lang = language("es-MX");
        let cfg = Config::default();
        assert_eq!(names(&cfg)[0], "Tomar agua");
        lang.switch("en");
        assert_eq!(names(&cfg)[0], "Drink water");
        lang.switch("ja");
        assert_ne!(names(&cfg)[0], "Drink water");
    }

    #[test]
    fn edited_names_stay_in_any_language() {
        let lang = language("es-MX");
        let mut cfg = Config::default();
        cfg.reminders[0].rename("Agua con limón");
        lang.switch("en");
        assert_eq!(names(&cfg)[0], "Agua con limón");
        // The other, unedited reminders still follow the language.
        assert_eq!(names(&cfg)[1], "Stretch your hands");
    }

    #[test]
    fn changing_minutes_is_not_an_edit() {
        let lang = language("es-MX");
        let mut cfg = Config::default();
        cfg.reminders[0].minutes = 45;
        cfg.reminders[0].enabled = false;
        lang.switch("en");
        assert_eq!(names(&cfg)[0], "Drink water");
        assert_eq!(cfg.reminders[0].name, None);
    }

    #[test]
    fn the_language_choice_survives_saving() {
        let dir = TempDir::new("lang-save");
        let path = dir.path().join("config.toml");
        let lang = language("es-MX");
        let mut cfg = Config::default();
        cfg.reminders[1].rename("Manos arriba");
        cfg.save_to(&path).unwrap();
        lang.switch("en");
        let Loaded::Existing(back) = Config::load_from(&path) else {
            panic!()
        };
        assert_eq!(names(&back)[0], "Drink water");
        assert_eq!(names(&back)[1], "Manos arriba");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("Tomar agua"),
            "preset names are never stored"
        );
    }

    #[test]
    fn a_new_reminder_is_named_in_the_language_until_renamed() {
        let lang = language("es-MX");
        let r = Reminder::preset(9, i18n::NEW_PRESET, 30, true);
        assert_eq!(r.display_name(), "Nuevo recordatorio");
        lang.switch("en");
        assert_eq!(r.display_name(), "New reminder");
    }

    #[test]
    fn next_id_never_reuses_an_id() {
        let mut cfg = Config::default();
        let max = cfg.reminders.iter().map(|r| r.id).max().unwrap();
        assert_eq!(cfg.next_id(), max + 1);
        cfg.reminders.clear();
        assert_eq!(cfg.next_id(), 1);
    }
}
