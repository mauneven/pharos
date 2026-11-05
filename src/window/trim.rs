//! Gives memory back once the window is up. Opening the window runs a lot of
//! GTK code that's never needed again (setup, parsing the theme, building the
//! widgets), and those pages stay mapped into Pharos for as long as it runs.
//! A moment after opening, hand the freed heap back and unmap the clean pages
//! of read-only files (library code, fonts). They stay in the page cache,
//! where the kernel can reclaim them, and whatever is still in use maps back
//! in on the next touch. Anything Pharos wrote to is left alone. Using the
//! window maps some of it back, so it trims again once you stop for a moment.

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

/// How long the window has to sit untouched before trimming again.
const IDLE: Duration = Duration::from_secs(3);

const MADV_DONTNEED: i32 = 4;

unsafe extern "C" {
    fn malloc_trim(pad: usize) -> i32;
    fn madvise(addr: *mut c_void, len: usize, advice: i32) -> i32;
}

/// Trims shortly after `window` opens, and again each time it goes idle.
pub fn watch(window: &impl IsA<gtk::Widget>) {
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let schedule = move || {
        if let Some(old) = pending.borrow_mut().take() {
            old.remove();
        }
        let slot = pending.clone();
        let id = glib::timeout_add_local_once(IDLE, move || {
            slot.borrow_mut().take();
            trim();
        });
        *pending.borrow_mut() = Some(id);
    };
    schedule();
    let events = gtk::EventControllerLegacy::new();
    events.set_propagation_phase(gtk::PropagationPhase::Capture);
    events.connect_event(move |_, _| {
        schedule();
        glib::Propagation::Proceed
    });
    window.add_controller(events);
}

fn trim() {
    // SAFETY: malloc_trim only returns free heap to the system.
    unsafe { malloc_trim(0) };
    let Ok(smaps) = std::fs::read_to_string("/proc/self/smaps") else {
        return;
    };
    for (start, end) in clean_file_mappings(&smaps) {
        // SAFETY: the range is a read-only file mapping holding no private
        // (written) pages, so dropping it loses nothing: the next access
        // reads the same bytes back from the file.
        unsafe { madvise(start as *mut c_void, end - start, MADV_DONTNEED) };
    }
}

/// From `/proc/self/smaps`: the read-only mappings of regular files that hold
/// no anonymous (written) pages.
fn clean_file_mappings(smaps: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut current: Option<(usize, usize)> = None;
    for line in smaps.lines() {
        if let Some(rest) = line.strip_prefix("Anonymous:") {
            if let Some(range) = current.take()
                && rest.trim().trim_end_matches("kB").trim() == "0"
            {
                out.push(range);
            }
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some((start, end)) = parts.next().and_then(|r| r.split_once('-')) else {
            continue;
        };
        let (Ok(start), Ok(end)) = (usize::from_str_radix(start, 16), usize::from_str_radix(end, 16))
        else {
            continue; // a field line such as "Rss:", not a mapping header
        };
        let perms = parts.next().unwrap_or("");
        let path = parts.nth(3).unwrap_or("");
        let regular_file = path.starts_with('/') && !path.starts_with("/dev/") && !path.starts_with("/memfd:");
        current = (regular_file && !perms.contains('w')).then_some((start, end));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_clean_read_only_files() {
        let smaps = "\
7f0000000000-7f0000001000 r-xp 00000000 00:1f 1234 /usr/lib/libgtk-4.so.1
Rss:                   4 kB
Anonymous:             0 kB
7f0000001000-7f0000002000 r--p 00001000 00:1f 1234 /usr/lib/libgtk-4.so.1
Anonymous:             4 kB
7f0000002000-7f0000003000 rw-p 00002000 00:1f 1234 /usr/lib/libgtk-4.so.1
Anonymous:             0 kB
7f0000003000-7f0000004000 rw-s 00000000 00:01 99 /memfd:gdk-wayland (deleted)
Anonymous:             0 kB
7f0000004000-7f0000005000 rw-p 00000000 00:00 0 [heap]
Anonymous:             0 kB
7f0000005000-7f0000006000 r--s 00000000 00:1f 77 /usr/share/fonts/TTF/Mono.ttf
Anonymous:             0 kB
";
        assert_eq!(
            clean_file_mappings(smaps),
            vec![(0x7f0000000000, 0x7f0000001000), (0x7f0000005000, 0x7f0000006000)]
        );
    }
}
