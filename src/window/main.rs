//! `pharos-window`: the Pharos window. The background process (`pharos`)
//! starts it when you open Pharos; it exits when the window closes, so GTK
//! only takes memory while you're looking at it.
//!
//! Arguments: none to show the window, `--toggle` to show or close it,
//! `--quit` to close it. A second start hands its arguments to the window
//! that's already open.

mod app;
mod hypr;
mod theme;

use gtk::prelude::*;
use gtk::{gio, glib};
use pharos_core::{config, paths};
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    // Pharos draws a handful of rows: GTK's software renderer handles that
    // with no visible difference and a fraction of the memory of the GPU
    // renderers. GSK_RENDERER=vulkan still picks another one.
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
        let cfg = match config::Config::load() {
            config::Loaded::FirstRun(cfg) | config::Loaded::Existing(cfg) => cfg,
        };
        *slot.borrow_mut() = Some(app::Pharos::new(app, cfg));
    });

    let slot = pharos.clone();
    app.connect_command_line(move |app, cmd| {
        let Some(p) = slot.borrow().clone() else {
            return glib::ExitCode::FAILURE;
        };
        let args: Vec<String> = cmd
            .arguments()
            .into_iter()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        match args.first().map(String::as_str) {
            None => p.present(),
            Some("--toggle") => p.toggle_window(),
            Some("--quit") => {
                p.flush();
                app.quit();
            }
            Some(other) => {
                eprintln!("pharos-window: unknown option {other}");
                return glib::ExitCode::FAILURE;
            }
        }
        glib::ExitCode::SUCCESS
    });

    let slot = pharos.clone();
    app.connect_shutdown(move |_| {
        if let Some(p) = slot.borrow().as_ref() {
            p.flush();
        }
    });

    app.run()
}
