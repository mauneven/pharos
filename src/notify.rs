//! Sends reminders through Omarchy's own notification command so they look
//! like every other Omarchy notification, with a Snooze button. Falls back to
//! `notify-send` outside Omarchy.

use std::process::{Command, Stdio};

use crate::i18n;

const GLYPH: &str = "\u{f009a}"; // nf-md-bell

pub fn send(id: u32, name: &str, minutes: u32) {
    let title = if name.trim().is_empty() { "Pharos" } else { name };
    let body = i18n::trf("notification_body", &[("n", &minutes.to_string())]);
    let snooze = i18n::tr("snooze");
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "pharos".into());

    let mut cmd = Command::new("omarchy-notification-send");
    cmd.args(["--app-name", "Pharos", "-g", GLYPH, "-u", "normal"])
        .arg(title)
        .arg(&body)
        .args(["--action", &snooze, &exe, "--snooze", &id.to_string()]);
    if spawn(cmd) {
        return;
    }

    let mut fallback = Command::new("notify-send");
    fallback
        .args(["-a", "Pharos", "-i", crate::paths::APP_ID])
        .arg(title)
        .arg(&body);
    if !spawn(fallback) {
        eprintln!("pharos: no notification command available ({title})");
    }
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
