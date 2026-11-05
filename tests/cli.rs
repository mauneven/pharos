//! Runs the real `pharos` binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn pharos() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pharos"))
}

struct Dir(PathBuf);

impl Dir {
    fn new(label: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("pharos-it-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn help_and_version() {
    let out = pharos().arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(text(&out).contains("Usage: pharos"));
    assert!(!text(&out).contains("--quit"));

    let out = pharos().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        text(&out).trim(),
        format!("pharos {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn mistakes_fail_in_the_terminal() {
    for args in [
        &["--bogus"][..],
        &["--snooze"],
        &["--snooze", "x"],
        &["--autostart", "maybe"],
    ] {
        let out = pharos().args(args).output().unwrap();
        assert!(!out.status.success(), "{args:?} should fail");
        assert!(text(&out).contains("pharos:"), "{args:?} explains why");
    }
}

#[test]
fn autostart_on_and_off() {
    let dir = Dir::new("autostart");
    let run = |args: &[&str]| {
        pharos()
            .args(args)
            .env("XDG_CONFIG_HOME", &dir.0)
            .output()
            .unwrap()
    };
    assert_eq!(text(&run(&["--autostart"])).trim(), "off");
    assert!(run(&["--autostart", "on"]).status.success());
    let entry = dir.0.join("autostart/com.mauneven.pharos.desktop");
    let body = std::fs::read_to_string(&entry).unwrap();
    assert!(body.contains(" --background"));
    assert_eq!(text(&run(&["--autostart"])).trim(), "on");
    assert!(run(&["--autostart", "off"]).status.success());
    assert!(!entry.exists());
}

fn wait_for(path: &Path, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(s) = std::fs::read_to_string(path) {
            if !s.is_empty() {
                return Some(s);
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    None
}

/// Starts Pharos for real with a one-minute reminder and waits for the
/// notification. Needs a graphical session and `dbus-run-session`, and takes
/// a minute, so it only runs on request: `cargo test -- --ignored`.
#[test]
#[ignore]
fn sends_the_notification_every_interval() {
    let dir = Dir::new("e2e");
    let config = dir.0.join("config");
    std::fs::create_dir_all(config.join("pharos")).unwrap();
    std::fs::write(
        config.join("pharos/config.toml"),
        "enabled = true\n[[reminder]]\nid = 3\npreset = \"water\"\nminutes = 1\nenabled = true\n",
    )
    .unwrap();
    std::fs::write(dir.0.join("language"), "es-MX\n").unwrap();

    // A stand-in for omarchy-notification-send that records its arguments.
    let bin = dir.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.0.join("sent.log");
    let fake = bin.join("omarchy-notification-send");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\necho --- >> '{}'\n",
            log.display(),
            log.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    // Its own process group, so Pharos and the private bus daemon go away
    // together however the test ends.
    use std::os::unix::process::CommandExt;
    let mut child = Command::new("dbus-run-session")
        .arg("--")
        .arg(env!("CARGO_BIN_EXE_pharos"))
        .arg("--background")
        .env("XDG_CONFIG_HOME", &config)
        .env("OMARCHY_LANGUAGE_FILE", dir.0.join("language"))
        .env("PATH", path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .expect("dbus-run-session");
    let group = child.id();
    let stop = || {
        let _ = Command::new("kill")
            .args(["-TERM", "--", &format!("-{group}")])
            .status();
    };

    let sent = wait_for(&log, Duration::from_secs(75));
    stop();
    let _ = child.wait();

    let sent = sent.expect("no notification within 75 seconds");
    let args: Vec<&str> = sent.lines().take_while(|l| *l != "---").collect();
    assert!(
        args.contains(&"Tomar agua"),
        "named in the user's language: {args:?}"
    );
    assert!(args.contains(&"Cada 1 min"), "{args:?}");
    let action = args.iter().position(|a| *a == "--action").unwrap();
    assert_eq!(args[action + 1], "Posponer 5 min");
    assert_eq!(&args[action + 3..], ["--snooze", "3"]);
}
