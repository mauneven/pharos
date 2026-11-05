//! The tray icon in the Omarchy bar (StatusNotifierItem).
//!
//! Left click opens the window. Right click: Open, Turn on / Turn off, Quit.
//! The icon is the lighthouse mark, drawn here in the bar's text color so it
//! sits with the other tray icons, and redrawn when the theme changes. It's
//! sent as pixels: the bar can't look up icons by name from the icon theme.
//!
//! ksni runs the D-Bus side on its own thread; menu clicks come back to the
//! background loop as `Action`s.

use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};

use pharos_core::{i18n, paths};

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    SetEnabled(bool),
    Quit,
}

struct Labels {
    open: String,
    turn_on: String,
    turn_off: String,
    quit: String,
}

impl Labels {
    fn current() -> Self {
        Labels {
            open: i18n::tr("tray_open"),
            turn_on: i18n::tr("tray_turn_on"),
            turn_off: i18n::tr("tray_turn_off"),
            quit: i18n::tr("tray_quit"),
        }
    }
}

/// The mark on Pharos's 24 grid, as filled rectangles (x0, y0, x1, y1): the
/// lantern frame open at the sides, the light, and its two beams. It's the
/// app icon's 2px square strokes, outlined.
const MARK: [(u32, u32, u32, u32); 9] = [
    (2, 2, 22, 4),   // top of the frame
    (2, 4, 4, 8),    // its sides
    (20, 4, 22, 8),
    (2, 20, 22, 22), // bottom of the frame
    (2, 16, 4, 20),
    (20, 16, 22, 20),
    (9, 9, 15, 15),  // the light
    (2, 11, 8, 13),  // beams
    (16, 11, 22, 13),
];

/// The mark in `rgb`, at 1x to 4x so the bar can pick a sharp one for any
/// scale. ARGB32, big-endian, as StatusNotifierItem wants it.
fn pixmaps(rgb: [u8; 3]) -> Vec<ksni::Icon> {
    (1..=4)
        .map(|scale| {
            let size = 24 * scale;
            let mut data = vec![0u8; (size * size * 4) as usize];
            for &(x0, y0, x1, y1) in &MARK {
                for y in y0 * scale..y1 * scale {
                    for x in x0 * scale..x1 * scale {
                        let i = ((y * size + x) * 4) as usize;
                        data[i..i + 4].copy_from_slice(&[255, rgb[0], rgb[1], rgb[2]]);
                    }
                }
            }
            ksni::Icon { width: size as i32, height: size as i32, data }
        })
        .collect()
}

/// The bar's text color, the way the Omarchy shell picks it: `[bar] text`
/// from the user's shell.toml, then the theme's, then the theme foreground.
pub fn bar_color() -> [u8; 3] {
    let theme = paths::omarchy_current().join("theme");
    let bar_text = |path: std::path::PathBuf| {
        let table: toml::Table = std::fs::read_to_string(path).ok()?.parse().ok()?;
        hex(table.get("bar")?.get("text")?.as_str()?)
    };
    let foreground = || {
        let text = std::fs::read_to_string(theme.join("colors.toml")).ok()?;
        let table: toml::Table = text.parse().ok()?;
        hex(table.get("foreground")?.as_str()?)
    };
    bar_text(paths::omarchy_user_shell_toml())
        .or_else(|| bar_text(theme.join("shell.toml")))
        .or_else(foreground)
        .unwrap_or([255, 255, 255])
}

/// "#rrggbb" (or "#rrggbbaa") to RGB; anything else, such as a role name
/// like "foreground", is None so the next source is tried.
fn hex(value: &str) -> Option<[u8; 3]> {
    let digits = value.trim().strip_prefix('#')?;
    if digits.len() != 6 && digits.len() != 8 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

struct Item {
    enabled: bool,
    color: [u8; 3],
    labels: Labels,
    tx: Box<dyn Fn(Action) + Send>,
}

impl Item {
    fn send(&self, action: Action) {
        (self.tx)(action);
    }
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "pharos".into()
    }

    fn title(&self) -> String {
        "Pharos".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        pixmaps(self.color)
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Pharos".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Action::Open);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let toggle = if self.enabled {
            &self.labels.turn_off
        } else {
            &self.labels.turn_on
        };
        vec![
            StandardItem {
                label: self.labels.open.clone(),
                activate: Box::new(|t: &mut Self| t.send(Action::Open)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: toggle.clone(),
                activate: Box::new(|t: &mut Self| t.send(Action::SetEnabled(!t.enabled))),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: self.labels.quit.clone(),
                activate: Box::new(|t: &mut Self| t.send(Action::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct Tray {
    handle: ksni::blocking::Handle<Item>,
}

impl Tray {
    /// Registers the tray icon. Returns None (and Pharos runs without one)
    /// when there's no session bus.
    pub fn spawn(enabled: bool, tx: impl Fn(Action) + Send + 'static) -> Option<Tray> {
        let item = Item {
            enabled,
            color: bar_color(),
            labels: Labels::current(),
            tx: Box::new(tx),
        };
        // The bar may start after Pharos at login; register anyway and let
        // the watcher pick the icon up when it appears.
        match item.assume_sni_available(true).spawn() {
            Ok(handle) => Some(Tray { handle }),
            Err(err) => {
                eprintln!("pharos: no tray icon: {err}");
                None
            }
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.handle.update(|item| item.enabled = enabled);
    }

    /// Redraws the icon in the bar's current text color, after a theme change.
    pub fn recolor(&self) {
        let color = bar_color();
        self.handle.update(|item| item.color = color);
    }

    /// Re-reads the menu labels after a language change.
    pub fn relabel(&self) {
        self.handle.update(|item| item.labels = Labels::current());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharos_core::testutil::language;
    use ksni::Tray as _;

    fn item(enabled: bool) -> (Item, std::sync::mpsc::Receiver<Action>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            Item {
                enabled,
                color: [255, 255, 255],
                labels: Labels::current(),
                tx: Box::new(move |a| {
                    let _ = tx.send(a);
                }),
            },
            rx,
        )
    }

    fn labels(item: &Item) -> Vec<String> {
        item.menu()
            .iter()
            .filter_map(|m| match m {
                MenuItem::Standard(s) => Some(s.label.clone()),
                _ => None,
            })
            .collect()
    }

    /// Clicks the n-th clickable entry and returns what it asked for.
    fn click(item: &mut Item, rx: &std::sync::mpsc::Receiver<Action>, n: usize) -> Action {
        let menu = item.menu();
        let entry = menu
            .into_iter()
            .filter_map(|m| match m {
                MenuItem::Standard(s) => Some(s),
                _ => None,
            })
            .nth(n)
            .unwrap();
        (entry.activate)(item);
        rx.try_recv().unwrap()
    }

    #[test]
    fn menu_has_open_toggle_quit() {
        let _l = language("en");
        assert_eq!(labels(&item(true).0), ["Open", "Turn off", "Quit"]);
        assert_eq!(labels(&item(false).0), ["Open", "Turn on", "Quit"]);
    }

    #[test]
    fn menu_follows_the_language() {
        let _l = language("es-MX");
        assert_eq!(labels(&item(true).0), ["Abrir", "Desactivar", "Salir"]);
        assert_eq!(labels(&item(false).0), ["Abrir", "Activar", "Salir"]);
    }

    #[test]
    fn entries_do_what_they_say() {
        let _l = language("en");
        let (mut on, rx) = item(true);
        assert_eq!(click(&mut on, &rx, 0), Action::Open);
        assert_eq!(click(&mut on, &rx, 1), Action::SetEnabled(false));
        assert_eq!(click(&mut on, &rx, 2), Action::Quit);
        let (mut off, rx) = item(false);
        assert_eq!(click(&mut off, &rx, 1), Action::SetEnabled(true));
    }

    #[test]
    fn left_click_opens() {
        let _l = language("en");
        let (mut it, rx) = item(true);
        it.activate(0, 0);
        assert_eq!(rx.try_recv().unwrap(), Action::Open);
    }

    #[test]
    fn draws_the_mark_in_the_bar_color() {
        let (mut it, _rx) = item(false);
        it.color = [0x4c, 0x4f, 0x69];
        assert!(it.icon_name().is_empty());
        let icons = it.icon_pixmap();
        assert_eq!(icons.iter().map(|i| i.width).collect::<Vec<_>>(), [24, 48, 72, 96]);
        let px = |icon: &ksni::Icon, x: i32, y: i32| {
            let i = ((y * icon.width + x) * 4) as usize;
            icon.data[i..i + 4].to_vec()
        };
        // At 2x: the light is drawn, the open side of the frame is empty.
        assert_eq!(px(&icons[1], 24, 24), [255, 0x4c, 0x4f, 0x69]);
        assert_eq!(px(&icons[1], 4, 24), [255, 0x4c, 0x4f, 0x69]); // a beam
        assert_eq!(px(&icons[1], 4, 18), [0, 0, 0, 0]);
        assert_eq!(px(&icons[1], 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn reads_hex_colors_only() {
        assert_eq!(hex("#ffffff"), Some([255, 255, 255]));
        assert_eq!(hex(" #4C4F69cc "), Some([0x4c, 0x4f, 0x69]));
        assert_eq!(hex("foreground"), None);
        assert_eq!(hex("#fff"), None);
    }
}
