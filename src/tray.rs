//! The tray icon in the Omarchy bar (StatusNotifierItem).
//!
//! Left click opens the window. Right click: Open, Turn on / Turn off, Quit.
//! The icon is a `-symbolic` theme icon, which the Omarchy bar tints with
//! its own text color, so it matches every theme. A paused Beacon shows the
//! light without its signal bars.
//!
//! ksni runs the D-Bus side on its own thread; menu clicks come back to the
//! GTK main loop through a channel as `Action`s.

use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};

use crate::{i18n, paths};

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
    tx: async_channel::Sender<Action>,
}

impl Item {
    fn send(&self, action: Action) {
        let _ = self.tx.send_blocking(action);
    }
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "beacon".into()
    }

    fn title(&self) -> String {
        "Beacon".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn icon_name(&self) -> String {
        if self.enabled {
            format!("{}-symbolic", paths::APP_ID)
        } else {
            format!("{}-paused-symbolic", paths::APP_ID)
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Beacon".into(),
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
    /// Registers the tray icon. Returns None (and Beacon runs without one)
    /// when there's no session bus.
    pub fn spawn(enabled: bool, tx: async_channel::Sender<Action>) -> Option<Tray> {
        let item = Item {
            enabled,
            labels: Labels::current(),
            tx,
        };
        // The bar may start after Beacon at login; register anyway and let
        // the watcher pick the icon up when it appears.
        match item.assume_sni_available(true).spawn() {
            Ok(handle) => Some(Tray { handle }),
            Err(err) => {
                eprintln!("beacon: no tray icon: {err}");
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
