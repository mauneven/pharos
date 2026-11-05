//! The background process: the clocks, the notifications and the tray icon.
//!
//! It loads no GUI toolkit, so it stays small all day. The window is a
//! separate program, `pharos-window`, started when you open it and gone
//! when you close it. The window edits the config file and says `reload`;
//! this process restarts the clocks that changed and publishes them in the
//! state file the window reads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use pharos_core::config::{self, Config};
use pharos_core::scheduler::Scheduler;
use pharos_core::{autostart, i18n, ipc, paths};

use crate::notify;
use crate::tray::{self, Tray};

const SNOOZE: Duration = Duration::from_secs(5 * 60);

enum Event {
    Tray(tray::Action),
    Command(String),
}

struct Daemon {
    config: Config,
    scheduler: Scheduler,
    tray: Option<Tray>,
    published: HashMap<u32, SystemTime>,
    config_mtime: Option<SystemTime>,
    language_mtime: Option<SystemTime>,
    theme_stamp: ThemeStamp,
}

/// What the tray icon's color depends on: the theme in use and the user's
/// shell.toml. A change to either redraws the icon.
type ThemeStamp = (Option<SystemTime>, Option<SystemTime>);

fn theme_stamp() -> ThemeStamp {
    (
        mtime(&paths::omarchy_current().join("theme.generation")),
        mtime(&paths::omarchy_user_shell_toml()),
    )
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Runs until Quit. `open` shows the window right away.
pub fn run(open: bool) -> ExitCode {
    let listener = match ipc::listen() {
        Ok(l) => l,
        Err(err) => {
            eprintln!("pharos: could not listen on {}: {err}", ipc::socket_path().display());
            return ExitCode::FAILURE;
        }
    };
    limit_malloc_arenas();
    i18n::reload();

    let config = match Config::load() {
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

    let (tx, rx) = mpsc::channel();
    let tray_tx = tx.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if let Some(cmd) = ipc::read_command(stream) {
                if tx.send(Event::Command(cmd)).is_err() {
                    return;
                }
            }
        }
    });

    let mut scheduler = Scheduler::default();
    scheduler.restart_all(&config, SystemTime::now());
    let mut d = Daemon {
        tray: Tray::spawn(config.enabled, move |a| {
            let _ = tray_tx.send(Event::Tray(a));
        }),
        config,
        scheduler,
        published: HashMap::new(),
        config_mtime: mtime(&paths::config_file()),
        language_mtime: mtime(&i18n::language_file()),
        theme_stamp: theme_stamp(),
    };
    d.publish();
    if open {
        window(&[]);
    }

    let mut next_tick = Instant::now() + Duration::from_secs(1);
    loop {
        let wait = next_tick.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(Event::Tray(tray::Action::Open)) => window(&[]),
            Ok(Event::Tray(tray::Action::SetEnabled(on))) => d.set_master(on),
            Ok(Event::Tray(tray::Action::Quit)) => break,
            Ok(Event::Command(cmd)) => match cmd.as_str() {
                "open" => window(&[]),
                "toggle" => window(&["--toggle"]),
                "quit" => break,
                "reload" => d.reload_config(),
                "background" | "ping" => {}
                other => match other.strip_prefix("snooze ").map(str::parse) {
                    Some(Ok(id)) => d.snooze(id),
                    _ => eprintln!("pharos: unknown command {other:?}"),
                },
            },
            Err(RecvTimeoutError::Timeout) => {
                next_tick += Duration::from_secs(1);
                // After a suspend, don't replay every missed second.
                if next_tick < Instant::now() {
                    next_tick = Instant::now() + Duration::from_secs(1);
                }
                d.tick();
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        d.publish();
    }

    if window_running() {
        window(&["--quit"]);
    }
    let _ = std::fs::remove_file(ipc::socket_path());
    let _ = std::fs::remove_file(ipc::state_path());
    ExitCode::SUCCESS
}

impl Daemon {
    fn tick(&mut self) {
        let now = SystemTime::now();
        for id in self.scheduler.tick(&self.config, now) {
            if let Some(r) = self.config.reminders.iter().find(|r| r.id == id) {
                notify::send(r.id, &r.display_name(), r.minutes);
            }
        }
        // Hand edits of the config file, and edits the window made while
        // its `reload` got lost, still land within a second.
        if mtime(&paths::config_file()) != self.config_mtime {
            self.reload_config();
        }
        let theme = theme_stamp();
        if theme != self.theme_stamp {
            self.theme_stamp = theme;
            if let Some(tray) = &self.tray {
                tray.recolor();
            }
        }
        let language = mtime(&i18n::language_file());
        if language != self.language_mtime {
            self.language_mtime = language;
            if i18n::reload() {
                if let Some(tray) = &self.tray {
                    tray.relabel();
                }
            }
        }
    }

    fn reload_config(&mut self) {
        self.config_mtime = mtime(&paths::config_file());
        let new = match Config::load() {
            config::Loaded::FirstRun(cfg) | config::Loaded::Existing(cfg) => cfg,
        };
        if new == self.config {
            return;
        }
        self.scheduler
            .apply(&self.config, &new, SystemTime::now());
        if new.enabled != self.config.enabled {
            if let Some(tray) = &self.tray {
                tray.set_enabled(new.enabled);
            }
        }
        self.config = new;
    }

    fn set_master(&mut self, on: bool) {
        let mut new = self.config.clone();
        new.enabled = on;
        self.scheduler
            .apply(&self.config, &new, SystemTime::now());
        self.config = new;
        if let Err(err) = self.config.save() {
            eprintln!("pharos: could not save config: {err}");
        }
        self.config_mtime = mtime(&paths::config_file());
        if let Some(tray) = &self.tray {
            tray.set_enabled(on);
        }
    }

    fn snooze(&mut self, id: u32) {
        self.scheduler.snooze(id, SNOOZE, SystemTime::now());
    }

    /// Rewrites the state file when a clock changed (not every second).
    fn publish(&mut self) {
        if self.scheduler.deadlines() != &self.published {
            self.published = self.scheduler.deadlines().clone();
            if let Err(err) = ipc::write_state(&self.published) {
                eprintln!("pharos: could not write {}: {err}", ipc::state_path().display());
            }
        }
    }
}

fn window_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("pharos-window"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("pharos-window"))
}

/// Starts `pharos-window` with `args`. GTK hands them to the window that's
/// already open, if there is one.
fn window(args: &[&str]) {
    let mut cmd = Command::new(window_exe());
    cmd.args(args).stdin(Stdio::null());
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(err) => eprintln!("pharos: could not open the window: {err}"),
    }
}

/// Whether a `pharos-window` is open, so Quit can close it.
fn window_running() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return false;
    };
    dir.flatten().any(|e| {
        std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == "pharos-window")
    })
}

/// glibc gives each thread its own malloc arena; this process allocates
/// almost nothing, so one shared arena saves memory for free.
fn limit_malloc_arenas() {
    unsafe extern "C" {
        fn mallopt(param: std::ffi::c_int, value: std::ffi::c_int) -> std::ffi::c_int;
    }
    const M_ARENA_MAX: std::ffi::c_int = -8;
    // SAFETY: a plain glibc tuning call, made before other threads start
    // allocating.
    unsafe {
        mallopt(M_ARENA_MAX, 1);
    }
}
