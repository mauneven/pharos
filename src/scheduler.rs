//! The whole job of Pharos: each enabled reminder has a clock. When it
//! reaches `minutes`, a notification goes out and the clock starts over.
//!
//! Changing a reminder's minutes, turning it on, or turning Pharos on starts
//! that clock from zero. If the machine slept (the tick saw a long gap), the
//! clocks restart quietly instead of firing a backlog of notifications.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::config::{Config, Reminder};

/// A tick gap longer than this means the machine was asleep.
const SLEEP_GAP: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Scheduler {
    deadlines: HashMap<u32, SystemTime>,
    last_tick: Option<SystemTime>,
}

fn interval(r: &Reminder) -> Duration {
    Duration::from_secs(u64::from(r.minutes) * 60)
}

impl Scheduler {
    /// Starts a reminder's clock from zero, or stops it when it's off.
    pub fn restart(&mut self, cfg: &Config, r: &Reminder, now: SystemTime) {
        if cfg.enabled && r.enabled {
            self.deadlines.insert(r.id, now + interval(r));
        } else {
            self.deadlines.remove(&r.id);
        }
    }

    pub fn restart_all(&mut self, cfg: &Config, now: SystemTime) {
        self.deadlines.clear();
        for r in &cfg.reminders {
            self.restart(cfg, r, now);
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.deadlines.remove(&id);
    }

    /// Pushes a running reminder's next notification `by` from now.
    pub fn snooze(&mut self, id: u32, by: Duration, now: SystemTime) {
        if let Some(deadline) = self.deadlines.get_mut(&id) {
            *deadline = now + by;
        }
    }

    /// Called once a second. Returns the reminders due now, and restarts
    /// their clocks.
    pub fn tick(&mut self, cfg: &Config, now: SystemTime) -> Vec<u32> {
        let slept = match self.last_tick.replace(now) {
            Some(last) => now
                .duration_since(last)
                .map(|gap| gap > SLEEP_GAP)
                .unwrap_or(true), // clock went backwards
            None => false,
        };
        if slept {
            self.restart_all(cfg, now);
            return Vec::new();
        }
        if !cfg.enabled {
            return Vec::new();
        }
        let mut due = Vec::new();
        for r in cfg.reminders.iter().filter(|r| r.enabled) {
            let deadline = self.deadlines.entry(r.id).or_insert(now + interval(r));
            if now >= *deadline {
                due.push(r.id);
                *deadline = now + interval(r);
            }
        }
        due
    }

    pub fn remaining(&self, id: u32, now: SystemTime) -> Option<Duration> {
        self.deadlines
            .get(&id)
            .map(|d| d.duration_since(now).unwrap_or(Duration::ZERO))
    }

    /// The reminder that fires next.
    pub fn next(&self, cfg: &Config, now: SystemTime) -> Option<(u32, Duration)> {
        cfg.reminders
            .iter()
            .filter(|r| cfg.enabled && r.enabled)
            .filter_map(|r| self.remaining(r.id, now).map(|d| (r.id, d)))
            .min_by_key(|(_, d)| *d)
    }
}

/// `12:04`, or `1:02:03` past an hour.
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            enabled: true,
            reminders: vec![
                Reminder::preset(1, "water", 20, true),
                Reminder::preset(2, "eyes", 10, false),
            ],
        }
    }

    /// Runs the scheduler second by second and collects (minute, id) fires.
    fn run(s: &mut Scheduler, cfg: &Config, start: SystemTime, secs: u64) -> Vec<(u64, u32)> {
        let mut fired = Vec::new();
        for t in 1..=secs {
            for id in s.tick(cfg, start + Duration::from_secs(t)) {
                fired.push((t / 60, id));
            }
        }
        fired
    }

    #[test]
    fn fires_every_interval_and_restarts() {
        let cfg = cfg();
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut s = Scheduler::default();
        s.restart_all(&cfg, start);
        s.tick(&cfg, start);
        let fired = run(&mut s, &cfg, start, 61 * 60);
        assert_eq!(fired, vec![(20, 1), (40, 1), (60, 1)]);
    }

    #[test]
    fn master_off_is_silent() {
        let mut cfg = cfg();
        cfg.enabled = false;
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut s = Scheduler::default();
        s.restart_all(&cfg, start);
        s.tick(&cfg, start);
        assert!(run(&mut s, &cfg, start, 30 * 60).is_empty());
    }

    #[test]
    fn sleep_restarts_quietly() {
        let cfg = cfg();
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut s = Scheduler::default();
        s.restart_all(&cfg, start);
        s.tick(&cfg, start);
        let wake = start + Duration::from_secs(3 * 3600);
        assert!(s.tick(&cfg, wake).is_empty());
        assert_eq!(s.remaining(1, wake), Some(Duration::from_secs(20 * 60)));
    }

    #[test]
    fn snooze_moves_the_next_fire() {
        let cfg = cfg();
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut s = Scheduler::default();
        s.restart_all(&cfg, start);
        s.snooze(1, Duration::from_secs(300), start);
        assert_eq!(s.remaining(1, start), Some(Duration::from_secs(300)));
        assert_eq!(s.remaining(2, start), None);
    }

    #[test]
    fn formats_clock() {
        assert_eq!(clock(Duration::from_secs(724)), "12:04");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
    }
}
