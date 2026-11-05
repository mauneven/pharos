//! Sends reminders through Omarchy's own notification command so they look
//! like every other Omarchy notification, with a Snooze button. Falls back to
//! `notify-send` outside Omarchy.

use std::process::{Command, Stdio};

use pharos_core::i18n;

const GLYPH: &str = "\u{f009a}"; // nf-md-bell

pub fn send(id: u32, name: &str, minutes: u32) {
    let title = if name.trim().is_empty() {
        "Pharos"
    } else {
        name
    };
    let body = i18n::trf("notification_body", &[("n", &minutes.to_string())]);
    let snooze = i18n::tr("snooze");
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "pharos".into());

    let mut cmd = Command::new("omarchy-notification-send");
    cmd.args(omarchy_args(id, title, &body, &snooze, &exe));
    if spawn(cmd) {
        return;
    }

    let mut fallback = Command::new("notify-send");
    fallback
        .args(["-a", "Pharos", "-i", pharos_core::paths::APP_ID])
        .arg(title)
        .arg(&body);
    if !spawn(fallback) {
        eprintln!("pharos: no notification command available ({title})");
    }
}

/// Arguments for `omarchy-notification-send`: the reminder's name and
/// interval, and a Snooze button that runs `pharos --snooze <id>`.
fn omarchy_args(id: u32, title: &str, body: &str, snooze: &str, exe: &str) -> Vec<String> {
    let mut args: Vec<String> = ["--app-name", "Pharos", "-g", GLYPH, "-u", "normal"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.extend([title.to_string(), body.to_string()]);
    args.extend([
        "--action".into(),
        snooze.into(),
        exe.into(),
        "--snooze".into(),
        id.to_string(),
    ]);
    args
}

/// Runs the command without blocking the UI and reaps it on a helper thread.
fn spawn(mut cmd: Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snooze_button_snoozes_this_reminder() {
        let args = omarchy_args(
            7,
            "Drink water",
            "Every 20 min",
            "Snooze 5 min",
            "/usr/bin/pharos",
        );
        let at = args.iter().position(|a| a == "--action").unwrap();
        assert_eq!(
            &args[at..],
            [
                "--action",
                "Snooze 5 min",
                "/usr/bin/pharos",
                "--snooze",
                "7"
            ]
        );
    }

    #[test]
    fn title_and_body_come_before_the_action() {
        // omarchy-notification-send reads <headline> [description] before
        // --action, and a headline that looks like a flag stays text.
        let args = omarchy_args(1, "--exec rm", "Every 1 min", "Snooze", "pharos");
        let title = args.iter().position(|a| a == "--exec rm").unwrap();
        let action = args.iter().position(|a| a == "--action").unwrap();
        assert_eq!(args[title + 1], "Every 1 min");
        assert!(title < action);
        assert_eq!(&args[..2], ["--app-name", "Pharos"]);
    }
}
