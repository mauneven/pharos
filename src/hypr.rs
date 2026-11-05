//! Opts Pharos's window in to Omarchy's window transparency, the way Flea
//! does: the window gets Hyprland's `transparent` tag, and Omarchy's own rule
//! for that tag applies the standard see-through look. That follows the
//! user's Window Transparency choice (Super+Backspace toggles it per window):
//! transparent by default, solid when opaque, frosted blur when glassy.

use std::process::Command;
use std::time::Duration;

use crate::paths;

/// Runs when the window opens: tags it transparent, then floats it in the
/// middle of the screen at `size` (logical px), the size its content needs.
/// Super+T still tiles it like any window. Runs off the main thread:
/// hyprctl blocks, and the window needs a moment to be mapped.
pub fn on_open(size: (i32, i32)) {
    std::thread::spawn(move || {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(80));
            let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
                return;
            };
            let pid = std::process::id();
            let ours = our_windows(&out.stdout, pid);
            if ours.is_empty() {
                continue;
            }
            for addr in untagged_windows(&out.stdout, pid) {
                tag(&addr);
            }
            for addr in ours {
                for lua in place(&addr, size) {
                    let _ = Command::new("hyprctl").args(["dispatch", &lua]).output();
                }
            }
            return;
        }
    });
}

/// The dispatches that float a window, size it and center it.
fn place(addr: &str, (w, h): (i32, i32)) -> [String; 3] {
    let window = format!("window = \"address:{addr}\"");
    [
        format!("hl.dsp.window.float({{ action = \"enable\", {window} }})"),
        format!("hl.dsp.window.resize({{ x = {w}, y = {h}, {window} }})"),
        format!("hl.dsp.window.center({{ {window} }})"),
    ]
}

/// From `hyprctl clients -j`: the addresses of this process's Pharos windows.
fn our_windows(json: &[u8], pid: u32) -> Vec<String> {
    let Ok(clients) = serde_json::from_slice::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    clients
        .iter()
        .filter(|c| {
            c["pid"].as_i64() == Some(i64::from(pid)) && c["class"].as_str() == Some(paths::APP_ID)
        })
        .filter_map(|c| c["address"].as_str().map(str::to_owned))
        .collect()
}

/// From `hyprctl clients -j`: the addresses of this process's Pharos
/// windows that don't carry the `transparent` tag yet.
fn untagged_windows(json: &[u8], pid: u32) -> Vec<String> {
    let Ok(clients) = serde_json::from_slice::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    clients
        .iter()
        .filter(|c| {
            c["pid"].as_i64() == Some(i64::from(pid)) && c["class"].as_str() == Some(paths::APP_ID)
        })
        .filter(|c| {
            // Already tagged (the window was shown before): leave it be.
            !c["tags"]
                .as_array()
                .is_some_and(|t| t.iter().any(|v| v.as_str() == Some("transparent")))
        })
        .filter_map(|c| c["address"].as_str().map(str::to_owned))
        .collect()
}

fn tag(addr: &str) {
    // Hyprland's Lua config takes Lua dispatches; older configs the classic
    // `tagwindow` dispatcher.
    let lua =
        format!("hl.dsp.window.tag({{ window = \"address:{addr}\", tag = \"+transparent\" }})");
    let ok = Command::new("hyprctl")
        .args(["dispatch", &lua])
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "ok");
    if !ok {
        let _ = Command::new("hyprctl")
            .args([
                "dispatch",
                "tagwindow",
                "+transparent",
                &format!("address:{addr}"),
            ])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_only_our_untagged_windows() {
        let json = br#"[
            {"address": "0xa", "pid": 42, "class": "com.mauneven.pharos", "tags": []},
            {"address": "0xb", "pid": 42, "class": "com.mauneven.pharos",
             "tags": ["transparent", "transparent-window*"]},
            {"address": "0xc", "pid": 7, "class": "com.mauneven.pharos", "tags": []},
            {"address": "0xd", "pid": 42, "class": "foot", "tags": []}
        ]"#;
        assert_eq!(untagged_windows(json, 42), vec!["0xa".to_string()]);
    }

    #[test]
    fn floats_centered_at_the_content_size() {
        let json = br#"[
            {"address": "0xa", "pid": 42, "class": "com.mauneven.pharos", "tags": ["transparent"]},
            {"address": "0xd", "pid": 42, "class": "foot", "tags": []}
        ]"#;
        assert_eq!(our_windows(json, 42), vec!["0xa".to_string()]);
        let lua = place("0xa", (560, 610));
        assert!(lua[0].contains("float") && lua[0].contains("address:0xa"));
        assert!(lua[1].contains("x = 560, y = 610"));
        assert!(lua[2].starts_with("hl.dsp.window.center("));
    }

    #[test]
    fn tolerates_bad_output() {
        assert!(untagged_windows(b"", 1).is_empty());
        assert!(untagged_windows(b"not json", 1).is_empty());
        assert!(untagged_windows(b"[]", 1).is_empty());
        assert!(our_windows(b"not json", 1).is_empty());
    }
}
