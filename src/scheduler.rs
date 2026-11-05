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

    // ------------------------------------------------------------------
    // A simulated clock: advances one second per tick, like the app does,
    // and records the minute (since the start) each reminder fired at.

    struct Sim {
        s: Scheduler,
        start: SystemTime,
        now: SystemTime,
    }

    impl Sim {
        fn new(cfg: &Config) -> Sim {
            let start = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000);
            let mut s = Scheduler::default();
            s.restart_all(cfg, start);
            s.tick(cfg, start);
            Sim {
                s,
                start,
                now: start,
            }
        }

        fn minutes(&self) -> u64 {
            self.now.duration_since(self.start).unwrap().as_secs() / 60
        }

        fn run(&mut self, cfg: &Config, minutes: u64) -> Vec<(u64, u32)> {
            let mut fired = Vec::new();
            for _ in 0..minutes * 60 {
                self.now += Duration::from_secs(1);
                for id in self.s.tick(cfg, self.now) {
                    fired.push((self.minutes(), id));
                }
            }
            fired
        }

        fn restart(&mut self, cfg: &Config, id: u32) {
            let r = cfg.reminders.iter().find(|r| r.id == id).unwrap();
            self.s.restart(cfg, r, self.now);
        }
    }

    fn one(id: u32, minutes: u32, enabled: bool) -> Reminder {
        Reminder::preset(id, "water", minutes, enabled)
    }

    fn config(reminders: Vec<Reminder>) -> Config {
        Config {
            enabled: true,
            reminders,
        }
    }

    #[test]
    fn never_fires_early() {
        let cfg = config(vec![one(1, 20, true)]);
        let mut sim = Sim::new(&cfg);
        // One second short of 20 minutes: nothing yet.
        for _ in 0..(20 * 60 - 1) {
            sim.now += Duration::from_secs(1);
            assert!(sim.s.tick(&cfg, sim.now).is_empty());
        }
        sim.now += Duration::from_secs(1);
        assert_eq!(sim.s.tick(&cfg, sim.now), vec![1]);
    }

    #[test]
    fn a_whole_day_has_the_right_count() {
        let cfg = config(vec![one(1, 20, true)]);
        let mut sim = Sim::new(&cfg);
        let fired = sim.run(&cfg, 8 * 60);
        assert_eq!(fired.len(), 24, "8 hours at 20 minutes is 24 reminders");
        assert!(fired.iter().all(|(m, _)| m % 20 == 0));
    }

    #[test]
    fn reminders_keep_their_own_intervals() {
        let cfg = config(vec![one(1, 10, true), one(2, 25, true)]);
        let mut sim = Sim::new(&cfg);
        let fired = sim.run(&cfg, 60);
        let at = |id| {
            fired
                .iter()
                .filter(|f| f.1 == id)
                .map(|f| f.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(at(1), vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(at(2), vec![25, 50]);
    }

    #[test]
    fn a_disabled_reminder_stays_silent() {
        let cfg = config(vec![one(1, 1, false)]);
        let mut sim = Sim::new(&cfg);
        assert!(sim.run(&cfg, 30).is_empty());
        assert_eq!(sim.s.remaining(1, sim.now), None);
    }

    #[test]
    fn changing_the_minutes_starts_a_new_interval_now() {
        let mut cfg = config(vec![one(1, 20, true)]);
        let mut sim = Sim::new(&cfg);
        assert!(sim.run(&cfg, 15).is_empty());
        cfg.reminders[0].minutes = 10;
        sim.restart(&cfg, 1);
        // Counted from minute 15, not from the start.
        assert_eq!(sim.run(&cfg, 30), vec![(25, 1), (35, 1), (45, 1)]);
    }

    #[test]
    fn turning_a_reminder_on_counts_from_that_moment() {
        let mut cfg = config(vec![one(1, 10, false)]);
        let mut sim = Sim::new(&cfg);
        assert!(sim.run(&cfg, 7).is_empty());
        cfg.reminders[0].enabled = true;
        sim.restart(&cfg, 1);
        assert_eq!(sim.run(&cfg, 20), vec![(17, 1), (27, 1)]);
    }

    #[test]
    fn turning_a_reminder_off_stops_it() {
        let mut cfg = config(vec![one(1, 10, true)]);
        let mut sim = Sim::new(&cfg);
        assert_eq!(sim.run(&cfg, 10), vec![(10, 1)]);
        cfg.reminders[0].enabled = false;
        sim.restart(&cfg, 1);
        assert!(sim.run(&cfg, 60).is_empty());
    }

    #[test]
    fn pausing_everything_and_resuming_restarts_every_clock() {
        let mut cfg = config(vec![one(1, 10, true), one(2, 30, true)]);
        let mut sim = Sim::new(&cfg);
        assert_eq!(sim.run(&cfg, 12), vec![(10, 1)]);
        cfg.enabled = false;
        sim.s.restart_all(&cfg, sim.now);
        assert!(sim.run(&cfg, 60).is_empty());
        assert_eq!(sim.s.next(&cfg, sim.now), None);
        cfg.enabled = true;
        sim.s.restart_all(&cfg, sim.now);
        // Back on at minute 72: both clocks start over from there.
        assert_eq!(
            sim.run(&cfg, 30),
            vec![(82, 1), (92, 1), (102, 1), (102, 2)]
        );
    }

    #[test]
    fn a_deleted_reminder_stops() {
        let mut cfg = config(vec![one(1, 5, true), one(2, 7, true)]);
        let mut sim = Sim::new(&cfg);
        sim.run(&cfg, 6);
        cfg.reminders.retain(|r| r.id != 1);
        sim.s.remove(1);
        let fired = sim.run(&cfg, 30);
        assert!(fired.iter().all(|(_, id)| *id == 2));
        assert_eq!(sim.s.remaining(1, sim.now), None);
    }

    #[test]
    fn a_new_reminder_gets_its_clock_on_the_next_tick() {
        let mut cfg = config(vec![]);
        let mut sim = Sim::new(&cfg);
        sim.run(&cfg, 3);
        // Added without an explicit restart: the tick schedules it.
        cfg.reminders.push(one(9, 4, true));
        assert_eq!(sim.run(&cfg, 10), vec![(7, 9), (11, 9)]);
    }

    #[test]
    fn snooze_replaces_the_next_fire_then_the_interval_resumes() {
        let cfg = config(vec![one(1, 20, true)]);
        let mut sim = Sim::new(&cfg);
        assert_eq!(sim.run(&cfg, 20), vec![(20, 1)]);
        sim.s.snooze(1, Duration::from_secs(5 * 60), sim.now);
        assert_eq!(sim.run(&cfg, 30), vec![(25, 1), (45, 1)]);
    }

    #[test]
    fn snoozing_a_stopped_reminder_does_nothing() {
        let cfg = config(vec![one(1, 20, false)]);
        let mut sim = Sim::new(&cfg);
        sim.s.snooze(1, Duration::from_secs(60), sim.now);
        assert!(sim.run(&cfg, 30).is_empty());
    }

    #[test]
    fn a_clock_going_backwards_restarts_quietly() {
        let cfg = config(vec![one(1, 10, true)]);
        let mut sim = Sim::new(&cfg);
        sim.run(&cfg, 9);
        sim.now -= Duration::from_secs(3600);
        assert!(sim.s.tick(&cfg, sim.now).is_empty());
        assert_eq!(sim.s.remaining(1, sim.now), Some(Duration::from_secs(600)));
    }

    #[test]
    fn short_hiccups_are_not_sleep() {
        // A busy machine can skip a few ticks; that's not a suspend.
        let cfg = config(vec![one(1, 1, true)]);
        let mut sim = Sim::new(&cfg);
        for _ in 0..2 {
            sim.now += Duration::from_secs(25);
            assert!(sim.s.tick(&cfg, sim.now).is_empty());
        }
        sim.now += Duration::from_secs(25);
        assert_eq!(sim.s.tick(&cfg, sim.now), vec![1]);
    }

    #[test]
    fn next_is_the_soonest_running_reminder() {
        let cfg = config(vec![one(1, 30, true), one(2, 5, true), one(3, 1, false)]);
        let sim = Sim::new(&cfg);
        assert_eq!(
            sim.s.next(&cfg, sim.now),
            Some((2, Duration::from_secs(300)))
        );
    }
}
