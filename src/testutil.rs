//! Helpers shared by the unit tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

/// A fresh directory under the system temp dir, removed when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "pharos-test-{}-{}-{label}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The UI language is process-wide, and tests run in parallel. Every test
/// that reads or changes it holds this lock, and gets English back after.
pub fn language(code: &str) -> LanguageGuard {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    crate::i18n::set_for_test(code);
    LanguageGuard(guard)
}

pub struct LanguageGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

impl LanguageGuard {
    pub fn switch(&self, code: &str) {
        crate::i18n::set_for_test(code);
    }
}

impl Drop for LanguageGuard {
    fn drop(&mut self) {
        crate::i18n::set_for_test("en");
    }
}
