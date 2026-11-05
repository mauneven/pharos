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

/// Reads the state file the background process publishes: id -> seconds
/// left (rounded).
fn clocks(dir: &Path) -> Vec<(u32, u64)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let text = std::fs::read_to_string(dir.join("pharos.state")).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let (id, ms) = l.split_once(' ')?;
            let ms: u64 = ms.parse().ok()?;
            Some((id.parse().ok()?, (ms.saturating_sub(now) + 500) / 1000))
        })
        .collect()
}

fn until(what: &str, timeout: Duration, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The background process on its own: no window and no GTK. It answers
/// on its socket, publishes the clocks, snoozes, follows config edits and
/// quits.
#[test]
fn background_process_keeps_the_clocks() {
    let dir = Dir::new("bg");
    let config = dir.0.join("config");
    std::fs::create_dir_all(config.join("pharos")).unwrap();
    let file = config.join("pharos/config.toml");
    std::fs::write(
        &file,
        "enabled = true\n[[reminder]]\nid = 3\npreset = \"water\"\nminutes = 20\nenabled = true\n\
         [[reminder]]\nid = 4\nname = \"Tea\"\nminutes = 30\nenabled = false\n",
    )
    .unwrap();
    let cmd = |args: &[&str]| {
        let mut c = pharos();
        c.args(args)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_RUNTIME_DIR", &dir.0)
            // No session bus: no tray icon in the real bar.
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        c
    };
    let mut daemon = cmd(&["--background"]).spawn().unwrap();

    until("the first clocks", Duration::from_secs(5), || {
        clocks(&dir.0) == [(3, 1200)]
    });

    assert!(cmd(&["--snooze", "3"]).status().unwrap().success());
    until("the snooze", Duration::from_secs(5), || clocks(&dir.0) == [(3, 300)]);

    // An edit to the file (what the window does): Tea on, new minutes for
    // water. Both clocks start over.
    let edited = std::fs::read_to_string(&file)
        .unwrap()
        .replace("minutes = 20", "minutes = 25")
        .replace("enabled = false", "enabled = true");
    std::fs::write(&file, edited).unwrap();
    until("the edit", Duration::from_secs(5), || {
        clocks(&dir.0) == [(3, 1500), (4, 1800)]
    });

    // Turning Pharos off stops every clock.
    let off = std::fs::read_to_string(&file)
        .unwrap()
        .replacen("enabled = true", "enabled = false", 1);
    std::fs::write(&file, off).unwrap();
    until("pausing", Duration::from_secs(5), || clocks(&dir.0).is_empty());

    assert!(cmd(&["--quit"]).status().unwrap().success());
    until("quit", Duration::from_secs(5), || {
        daemon.try_wait().unwrap().is_some()
    });
    assert!(!dir.0.join("pharos.sock").exists());

    // With nothing running, a snooze says so.
    let out = cmd(&["--snooze", "3"]).stderr(std::process::Stdio::piped()).output().unwrap();
    assert!(!out.status.success());
}

/// Starts Pharos for real with a one-minute reminder and waits for the
/// notification. Takes a minute, so it only runs on request:
/// `cargo test -- --ignored`.
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
        .env("XDG_RUNTIME_DIR", &dir.0)
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
