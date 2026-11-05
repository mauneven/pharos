//! Beacon: interval break reminders for Omarchy.
//!
//! One process runs per session. Launching `beacon` again shows its window;
//! `--snooze` and `--toggle` reach the running instance over D-Bus.

mod app;
mod autostart;
mod config;
mod i18n;
mod notify;
mod paths;
mod scheduler;
mod theme;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const HELP: &str = "\
Beacon — break reminders for Omarchy

Usage: beacon [option]

  (none)              Open the window (starts Beacon if it isn't running)
  --background        Start without opening the window
  --toggle            Show or hide the window
  --snooze <id>       Push a reminder's next notification 5 minutes out
  --quit              Stop the running Beacon
  --autostart on|off  Start Beacon at login, or stop doing so
  -V, --version       Print the version
  -h, --help          Print this help

Every reminder fires each N minutes, then its clock starts over.
Reminders live in ~/.config/beacon/config.toml.";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Commands that don't need the running instance.
    match args.get(1).map(String::as_str) {
        Some("-h" | "--help") => {
            println!("{HELP}");
            return glib::ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("beacon {}", env!("CARGO_PKG_VERSION"));
            return glib::ExitCode::SUCCESS;
        }
        Some("--autostart") => {
            return match args.get(2).map(String::as_str) {
                Some(v @ ("on" | "off")) => match autostart::set(v == "on") {
                    Ok(()) => glib::ExitCode::SUCCESS,
                    Err(err) => {
                        eprintln!("beacon: {err}");
                        glib::ExitCode::FAILURE
                    }
                },
                _ => {
                    println!("{}", if autostart::enabled() { "on" } else { "off" });
                    glib::ExitCode::SUCCESS
                }
            };
        }
        None => {}
        Some("--background" | "--toggle" | "--quit") if args.len() == 2 => {}
        Some("--snooze") if args.len() == 3 && args[2].parse::<u32>().is_ok() => {}
        Some("--snooze") => {
            eprintln!("beacon: --snooze needs a reminder id");
            return glib::ExitCode::FAILURE;
        }
        Some(other) => {
            eprintln!("beacon: unknown option {other}\n\n{HELP}");
            return glib::ExitCode::FAILURE;
        }
    }

    let app = gtk::Application::builder()
        .application_id(paths::APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let beacon: Rc<RefCell<Option<Rc<app::Beacon>>>> = Rc::default();

    let slot = beacon.clone();
    app.connect_startup(move |app| {
        let config = match config::Config::load() {
            config::Loaded::FirstRun(cfg) => {
                // A reminder app only works while it runs, so the first run
                // turns on start-at-login. The pencil view can turn it off.
                if let Err(err) = autostart::set(true) {
                    eprintln!("beacon: could not enable autostart: {err}");
                }
                cfg
            }
            config::Loaded::Existing(cfg) => cfg,
        };
        *slot.borrow_mut() = Some(app::Beacon::new(app, config));
    });

    let slot = beacon.clone();
    app.connect_command_line(move |app, cmd| {
        let Some(b) = slot.borrow().clone() else {
            return glib::ExitCode::FAILURE;
        };
        let argv: Vec<String> = cmd
            .arguments()
            .into_iter()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        match argv.first().map(String::as_str) {
            None => b.present(),
            Some("--background") => {}
            Some("--toggle") => b.toggle_window(),
            Some("--quit") => {
                b.flush();
                app.quit();
            }
            Some("--snooze") => {
                if let Some(id) = argv.get(1).and_then(|s| s.parse().ok()) {
                    b.snooze(id);
                }
            }
            Some(_) => return glib::ExitCode::FAILURE,
        }
        glib::ExitCode::SUCCESS
    });

    let slot = beacon.clone();
    app.connect_shutdown(move |_| {
        if let Some(b) = slot.borrow().as_ref() {
            b.flush();
        }
    });

    app.run_with_args(&args)
}
