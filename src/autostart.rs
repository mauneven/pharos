//! Start at login through XDG autostart, which Omarchy's uwsm session runs.

use std::path::Path;

use crate::paths;

pub fn enabled() -> bool {
    paths::autostart_file().is_file()
}

pub fn set(on: bool) -> std::io::Result<()> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "pharos".into());
    set_at(&paths::autostart_file(), on, &exe)
}

/// Writes or removes the autostart entry at `path`, launching `exe`.
fn set_at(path: &Path, on: bool, exe: &str) -> std::io::Result<()> {
    if !on {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, entry(exe))
}

fn entry(exe: &str) -> String {
    let exec = if exe.contains(char::is_whitespace) {
        format!("\"{exe}\"")
    } else {
        exe.to_string()
    };
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Pharos\n\
         Comment=Break reminders\n\
         Exec={exec} --background\n\
         Icon={id}\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        id = paths::APP_ID
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_starts_in_the_background() {
        let e = entry("/usr/bin/pharos");
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("Exec=/usr/bin/pharos --background\n"));
        assert!(e.contains("Icon=com.mauneven.pharos\n"));
        assert!(e.contains("NoDisplay=true"));
    }

    #[test]
    fn quotes_paths_with_spaces() {
        assert!(entry("/opt/my apps/pharos").contains("Exec=\"/opt/my apps/pharos\" --background"));
    }

    #[test]
    fn turns_on_and_off() {
        let dir = crate::testutil::TempDir::new("autostart");
        let path = dir.path().join("autostart/pharos.desktop");
        set_at(&path, true, "/usr/bin/pharos").unwrap();
        assert!(path.is_file());
        set_at(&path, false, "/usr/bin/pharos").unwrap();
        assert!(!path.exists());
        // Turning off twice is fine.
        set_at(&path, false, "/usr/bin/pharos").unwrap();
    }
}
