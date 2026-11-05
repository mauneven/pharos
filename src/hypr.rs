//! Opts Pharos's window in to Omarchy's window transparency, the way Flea
//! does: the window gets Hyprland's `transparent` tag, and Omarchy's own rule
//! for that tag applies the standard see-through look. That follows the
//! user's Window Transparency choice (Super+Backspace toggles it per window):
//! transparent by default, solid when opaque, frosted blur when glassy.

use std::process::Command;
use std::time::Duration;

use crate::paths;

/// Tags every window of this process. Runs off the main thread: hyprctl
/// blocks, and the window needs a moment to be mapped.
pub fn tag_transparent() {
    std::thread::spawn(|| {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(80));
            let addresses = our_windows();
            if !addresses.is_empty() {
                for addr in addresses {
                    tag(&addr);
                }
                return;
            }
        }
    });
}

fn our_windows() -> Vec<String> {
    let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
        return Vec::new();
    };
    untagged_windows(&out.stdout, std::process::id())
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
    fn tolerates_bad_output() {
        assert!(untagged_windows(b"", 1).is_empty());
        assert!(untagged_windows(b"not json", 1).is_empty());
        assert!(untagged_windows(b"[]", 1).is_empty());
    }
}
