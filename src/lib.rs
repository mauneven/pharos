//! What both Pharos programs share: the config file, the clocks, the
//! translations, and the way they talk to each other.
//!
//! `pharos` runs in the background with no GUI toolkit loaded: it keeps the
//! clocks, sends the notifications and shows the tray icon. `pharos-window`
//! is the GTK window; it only runs while the window is open.

pub mod autostart;
pub mod config;
pub mod i18n;
pub mod ipc;
pub mod paths;
pub mod scheduler;
#[doc(hidden)]
pub mod testutil;
