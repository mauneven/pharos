//! Start at login through XDG autostart, which Omarchy's uwsm session runs.

use crate::paths;

pub fn enabled() -> bool {
    paths::autostart_file().is_file()
}

pub fn set(on: bool) -> std::io::Result<()> {
    let path = paths::autostart_file();
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "beacon".into());
    let exec = if exe.contains(char::is_whitespace) {
        format!("\"{exe}\"")
    } else {
        exe
    };
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Beacon\n\
             Comment=Break reminders\n\
             Exec={exec} --background\n\
             Icon={id}\n\
             NoDisplay=true\n\
             X-GNOME-Autostart-enabled=true\n",
            id = paths::APP_ID
        ),
    )
}
