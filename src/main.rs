//! `pharos`: the background half of Pharos. It keeps the clocks, sends the
//! notifications and shows the tray icon, without loading GTK. The window
//! is `pharos-window`, started on demand.
//!
//! One background process runs per session. Running `pharos` again, or
//! `pharos --toggle` / `--snooze`, sends the command to it over a socket.

mod cli;
mod daemon;
mod notify;
mod tray;

use pharos_core::{autostart, ipc};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&args) {
        Err(err) => {
            eprintln!("pharos: {err}\n\n{}", cli::HELP);
            return ExitCode::FAILURE;
        }
        Ok(cmd) => cmd,
    };

    // What the running instance should hear, if one is running.
    let message = match command {
        cli::Command::Help => {
            println!("{}", cli::HELP);
            return ExitCode::SUCCESS;
        }
        cli::Command::Version => {
            println!("pharos {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        cli::Command::Autostart(None) => {
            println!("{}", if autostart::enabled() { "on" } else { "off" });
            return ExitCode::SUCCESS;
        }
        cli::Command::Autostart(Some(on)) => {
            return match autostart::set(on) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("pharos: {err}");
                    ExitCode::FAILURE
                }
            };
        }
        cli::Command::Open => "open".to_string(),
        cli::Command::Background => "background".to_string(),
        cli::Command::Toggle => "toggle".to_string(),
        cli::Command::Quit => "quit".to_string(),
        cli::Command::Snooze(id) => format!("snooze {id}"),
    };

    if ipc::send(&message).is_ok() {
        return ExitCode::SUCCESS;
    }
    // Nothing running yet: this process becomes the background one.
    match command {
        cli::Command::Open | cli::Command::Toggle => daemon::run(true),
        cli::Command::Background => daemon::run(false),
        cli::Command::Quit => ExitCode::SUCCESS,
        _ => {
            eprintln!("pharos: Pharos isn't running");
            ExitCode::FAILURE
        }
    }
}
