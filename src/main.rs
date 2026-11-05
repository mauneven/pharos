//! Pharos: interval break reminders for Omarchy.
//!
//! One process runs per session. Launching `pharos` again shows its window;
//! `--snooze` and `--toggle` reach the running instance over D-Bus.

mod app;
mod autostart;
mod cli;
mod config;
mod hypr;
mod i18n;
mod notify;
mod paths;
mod scheduler;
#[cfg(test)]
mod testutil;
mod theme;
mod tray;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Commands that don't need the running instance run here; the rest are
    // checked here so mistakes print in your terminal, then forwarded.
    match cli::parse(&args[1..]) {
        Err(err) => {
            eprintln!("pharos: {err}\n\n{}", cli::HELP);
            return glib::ExitCode::FAILURE;
        }
        Ok(cli::Command::Help) => {
            println!("{}", cli::HELP);
            return glib::ExitCode::SUCCESS;
        }
        Ok(cli::Command::Version) => {
            println!("pharos {}", env!("CARGO_PKG_VERSION"));
            return glib::ExitCode::SUCCESS;
        }
        Ok(cli::Command::Autostart(None)) => {
            println!("{}", if autostart::enabled() { "on" } else { "off" });
            return glib::ExitCode::SUCCESS;
        }
        Ok(cli::Command::Autostart(Some(on))) => {
            return match autostart::set(on) {
                Ok(()) => glib::ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("pharos: {err}");
                    glib::ExitCode::FAILURE
                }
            };
        }
        Ok(_) => {}
    }

    // Pharos draws a handful of rows: GTK's software renderer handles that
    // with no visible difference and about a third of the memory of the GPU
    // renderers. GSK_RENDERER=vulkan pharos still picks another one.
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: no other threads exist yet.
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
    }

    let app = gtk::Application::builder()
        .application_id(paths::APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let pharos: Rc<RefCell<Option<Rc<app::Pharos>>>> = Rc::default();

    let slot = pharos.clone();
    app.connect_startup(move |app| {
        let config = match config::Config::load() {
            config::Loaded::FirstRun(cfg) => {
                // A reminder app only works while it runs, so the first run
                // turns on start-at-login. The pencil view can turn it off.
                if let Err(err) = autostart::set(true) {
                    eprintln!("pharos: could not enable autostart: {err}");
                }
                cfg
            }
            config::Loaded::Existing(cfg) => cfg,
        };
        *slot.borrow_mut() = Some(app::Pharos::new(app, config));
    });

    let slot = pharos.clone();
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
        match cli::parse(&argv) {
            Ok(cli::Command::Open) => b.present(),
            Ok(cli::Command::Background) => {}
            Ok(cli::Command::Toggle) => b.toggle_window(),
            Ok(cli::Command::Quit) => {
                b.flush();
                app.quit();
            }
            Ok(cli::Command::Snooze(id)) => b.snooze(id),
            _ => return glib::ExitCode::FAILURE,
        }
        glib::ExitCode::SUCCESS
    });

    let slot = pharos.clone();
    app.connect_shutdown(move |_| {
        if let Some(b) = slot.borrow().as_ref() {
            b.flush();
        }
    });

    app.run_with_args(&args)
}
