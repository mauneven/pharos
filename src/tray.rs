//! The tray icon in the Omarchy bar (StatusNotifierItem).
//!
//! Left click opens the window. Right click: Open, Turn on / Turn off, Quit.
//! The icon is the app icon itself, the lighthouse.
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

struct Item {
    enabled: bool,
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

    fn icon_name(&self) -> String {
        paths::APP_ID.into()
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
    fn shows_the_app_icon() {
        let _l = language("en");
        let (it, _rx) = item(false);
        assert_eq!(it.icon_name(), paths::APP_ID);
        assert_eq!(it.id(), "pharos");
    }
}
