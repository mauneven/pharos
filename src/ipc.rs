//! How the two programs talk.
//!
//! - Commands (`open`, `toggle`, `snooze 3`, `reload`...) go to the
//!   background process over a Unix socket, one line per connection.
//! - The clocks come back in a small state file the background process
//!   rewrites whenever a deadline changes: one `id unix-millis` line per
//!   running reminder. The window watches it to show the countdowns.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("pharos.sock")
}

pub fn state_path() -> PathBuf {
    runtime_dir().join("pharos.state")
}

/// Sends one command to the running background process. Fails when none
/// is running.
pub fn send(command: &str) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")
}

/// Takes the socket for this process. A socket file nobody answers on is
/// left over from a crash, and is replaced.
pub fn listen() -> std::io::Result<UnixListener> {
    let path = socket_path();
    match UnixListener::bind(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            if UnixStream::connect(&path).is_ok() {
                return Err(err);
            }
            std::fs::remove_file(&path)?;
            UnixListener::bind(&path)
        }
        other => other,
    }
}

/// Reads the one command a client sent.
pub fn read_command(stream: UnixStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut line = String::new();
    stream.take(256).read_to_string(&mut line).ok()?;
    let line = line.trim();
    (!line.is_empty()).then(|| line.to_string())
}

fn millis(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

pub fn encode_state(deadlines: &HashMap<u32, SystemTime>) -> String {
    let mut ids: Vec<_> = deadlines.keys().copied().collect();
    ids.sort_unstable();
    ids.iter()
        .map(|id| format!("{id} {}\n", millis(deadlines[id])))
        .collect()
}

pub fn decode_state(text: &str) -> HashMap<u32, SystemTime> {
    text.lines()
        .filter_map(|line| {
            let (id, ms) = line.split_once(' ')?;
            let ms: u64 = ms.trim().parse().ok()?;
            Some((id.parse().ok()?, UNIX_EPOCH + Duration::from_millis(ms)))
        })
        .collect()
}

pub fn write_state(deadlines: &HashMap<u32, SystemTime>) -> std::io::Result<()> {
    let path = state_path();
    let tmp = path.with_extension("state.tmp");
    std::fs::write(&tmp, encode_state(deadlines))?;
    std::fs::rename(tmp, path)
}

pub fn read_state() -> Option<HashMap<u32, SystemTime>> {
    std::fs::read_to_string(state_path())
        .ok()
        .map(|t| decode_state(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        let t = UNIX_EPOCH + Duration::from_millis(1_790_000_000_123);
        let mut d = HashMap::new();
        d.insert(3, t);
        d.insert(1, t + Duration::from_secs(60));
        let text = encode_state(&d);
        assert_eq!(text, "1 1790000060123\n3 1790000000123\n");
        assert_eq!(decode_state(&text), d);
    }

    #[test]
    fn bad_state_lines_are_skipped() {
        let d = decode_state("x 1\n2 nope\n\n4 1000\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[&4], UNIX_EPOCH + Duration::from_secs(1));
    }
}
