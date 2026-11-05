//! Command-line parsing for `pharos`. Commands meant for the running
//! instance are forwarded to it over its socket.

pub const HELP: &str = "\
Pharos — break reminders for Omarchy

Usage: pharos [option]

  (none)              Open the window (starts Pharos if it isn't running)
  --background        Start without opening the window
  --toggle            Show or hide the window
  --snooze <id>       Push a reminder's next notification 5 minutes out
  --autostart on|off  Start Pharos at login, or stop doing so
  -V, --version       Print the version
  -h, --help          Print this help

Every reminder fires each N minutes, then its clock starts over.
Reminders live in ~/.config/pharos/config.toml.";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    /// `Some(on)` sets it, `None` prints whether it's on.
    Autostart(Option<bool>),
    Open,
    Background,
    Toggle,
    /// Not in the help: the tray's Quit does this, and scripts may need it.
    Quit,
    Snooze(u32),
}

/// Parses the arguments after the program name.
pub fn parse<S: AsRef<str>>(args: &[S]) -> Result<Command, String> {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let only = |cmd: Command| {
        if args.len() == 1 {
            Ok(cmd)
        } else {
            Err(format!("{} takes no arguments", args[0]))
        }
    };
    match args.as_slice() {
        [] => Ok(Command::Open),
        ["-h" | "--help", ..] => Ok(Command::Help),
        ["-V" | "--version", ..] => Ok(Command::Version),
        ["--autostart"] => Ok(Command::Autostart(None)),
        ["--autostart", "on"] => Ok(Command::Autostart(Some(true))),
        ["--autostart", "off"] => Ok(Command::Autostart(Some(false))),
        ["--autostart", ..] => Err("--autostart takes on or off".into()),
        ["--background", ..] => only(Command::Background),
        ["--toggle", ..] => only(Command::Toggle),
        ["--quit", ..] => only(Command::Quit),
        ["--snooze", id] => id
            .parse()
            .map(Command::Snooze)
            .map_err(|_| format!("--snooze needs a reminder id, not {id:?}")),
        ["--snooze", ..] => Err("--snooze needs a reminder id".into()),
        [other, ..] => Err(format!("unknown option {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args)
    }

    #[test]
    fn no_arguments_opens_the_window() {
        assert_eq!(p(&[]), Ok(Command::Open));
    }

    #[test]
    fn known_commands() {
        assert_eq!(p(&["--help"]), Ok(Command::Help));
        assert_eq!(p(&["-h"]), Ok(Command::Help));
        assert_eq!(p(&["-V"]), Ok(Command::Version));
        assert_eq!(p(&["--background"]), Ok(Command::Background));
        assert_eq!(p(&["--toggle"]), Ok(Command::Toggle));
        assert_eq!(p(&["--quit"]), Ok(Command::Quit));
        assert_eq!(p(&["--snooze", "3"]), Ok(Command::Snooze(3)));
        assert_eq!(p(&["--autostart"]), Ok(Command::Autostart(None)));
        assert_eq!(
            p(&["--autostart", "on"]),
            Ok(Command::Autostart(Some(true)))
        );
        assert_eq!(
            p(&["--autostart", "off"]),
            Ok(Command::Autostart(Some(false)))
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(p(&["--bogus"]).is_err());
        assert!(p(&["--snooze"]).is_err());
        assert!(p(&["--snooze", "abc"]).is_err());
        assert!(p(&["--snooze", "-1"]).is_err());
        assert!(p(&["--snooze", "1", "2"]).is_err());
        assert!(p(&["--autostart", "maybe"]).is_err());
        assert!(p(&["--toggle", "now"]).is_err());
    }

    #[test]
    fn help_does_not_advertise_quit() {
        assert!(!HELP.contains("--quit"));
    }
}
