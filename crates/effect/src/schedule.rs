//! [`Schedule`] — a pure value describing *when* (and whether) to
//! repeat an effect.
//!
//! A `Schedule` is a state machine. Each `step()` either says
//! "continue after this delay (here's the next schedule)" or "stop".
//! State lives in the captured closure of the next schedule produced
//! by `step()`, which makes composition just a matter of wrapping.
//!
//! Use schedules with [`Effect::retry`](crate::Effect::retry) for
//! recovering from failure, and
//! [`Effect::repeat`](crate::Effect::repeat) for polling or batching
//! work.

use std::sync::Arc;
use std::time::Duration;

/// A description of when (and whether) to repeat an effect.
pub struct Schedule {
    step_fn: Arc<dyn Fn() -> ScheduleStep + Send + Sync>,
}

impl Clone for Schedule {
    fn clone(&self) -> Self {
        Schedule { step_fn: self.step_fn.clone() }
    }
}

/// One step in a schedule's evolution.
pub enum ScheduleStep {
    /// Continue after waiting `Duration`. The included `Schedule` is the
    /// schedule's state *after* this step.
    Continue(Duration, Schedule),
    /// Stop iterating.
    Done,
}

impl Schedule {
    /// Stop immediately.
    pub fn stop() -> Self {
        Schedule { step_fn: Arc::new(|| ScheduleStep::Done) }
    }

    /// Continue exactly once with no delay, then stop.
    pub fn once() -> Self {
        Schedule {
            step_fn: Arc::new(|| ScheduleStep::Continue(Duration::ZERO, Schedule::stop())),
        }
    }

    /// Continue forever with no delay between iterations.
    pub fn forever() -> Self {
        Schedule {
            step_fn: Arc::new(|| ScheduleStep::Continue(Duration::ZERO, Schedule::forever())),
        }
    }

    /// Continue exactly `n` times with no delay, then stop.
    ///
    /// **Note:** when used with `Effect::retry`, this means "up to `n`
    /// extra attempts after the first" — so the effect runs at most
    /// `n + 1` times.
    pub fn recurs(n: u64) -> Self {
        Self::recurs_from(n)
    }

    fn recurs_from(remaining: u64) -> Self {
        Schedule {
            step_fn: Arc::new(move || {
                if remaining == 0 {
                    ScheduleStep::Done
                } else {
                    ScheduleStep::Continue(Duration::ZERO, Self::recurs_from(remaining - 1))
                }
            }),
        }
    }

    /// Continue forever with a fixed delay between iterations.
    pub fn spaced(delay: Duration) -> Self {
        Schedule {
            step_fn: Arc::new(move || {
                ScheduleStep::Continue(delay, Self::spaced(delay))
            }),
        }
    }

    /// Continue forever with exponentially-growing delays starting at
    /// `base` (doubles each iteration, saturating at `Duration::MAX`).
    pub fn exponential(base: Duration) -> Self {
        Self::exponential_from(base)
    }

    fn exponential_from(current: Duration) -> Self {
        Schedule {
            step_fn: Arc::new(move || {
                let next = current.checked_mul(2).unwrap_or(Duration::MAX);
                ScheduleStep::Continue(current, Self::exponential_from(next))
            }),
        }
    }

    /// Continue forever with Fibonacci-spaced delays starting at `base`.
    pub fn fibonacci(base: Duration) -> Self {
        Self::fibonacci_from(base, base)
    }

    fn fibonacci_from(prev: Duration, current: Duration) -> Self {
        Schedule {
            step_fn: Arc::new(move || {
                let next = prev.checked_add(current).unwrap_or(Duration::MAX);
                ScheduleStep::Continue(current, Self::fibonacci_from(current, next))
            }),
        }
    }

    /// Take one step.
    pub fn step(&self) -> ScheduleStep {
        (self.step_fn)()
    }

    /// Cap this schedule at `n` iterations. The wrapped schedule's
    /// delays are preserved; only the *count* is bounded.
    pub fn max_attempts(self, n: u64) -> Self {
        Self::max_attempts_from(self, n)
    }

    fn max_attempts_from(inner: Schedule, remaining: u64) -> Self {
        Schedule {
            step_fn: Arc::new(move || {
                if remaining == 0 {
                    return ScheduleStep::Done;
                }
                match inner.step() {
                    ScheduleStep::Done => ScheduleStep::Done,
                    ScheduleStep::Continue(d, next) => {
                        ScheduleStep::Continue(d, Self::max_attempts_from(next, remaining - 1))
                    }
                }
            }),
        }
    }

    /// Stop after the *cumulative* delay would exceed `max_total`.
    pub fn bounded(self, max_total: Duration) -> Self {
        Self::bounded_from(self, max_total, Duration::ZERO)
    }

    fn bounded_from(inner: Schedule, max: Duration, elapsed: Duration) -> Self {
        Schedule {
            step_fn: Arc::new(move || {
                if elapsed >= max {
                    return ScheduleStep::Done;
                }
                match inner.step() {
                    ScheduleStep::Done => ScheduleStep::Done,
                    ScheduleStep::Continue(d, next) => {
                        let new_elapsed = elapsed.saturating_add(d);
                        if new_elapsed > max {
                            ScheduleStep::Done
                        } else {
                            ScheduleStep::Continue(d, Self::bounded_from(next, max, new_elapsed))
                        }
                    }
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_delays(mut s: Schedule, max: usize) -> Vec<Duration> {
        let mut out = Vec::new();
        for _ in 0..max {
            match s.step() {
                ScheduleStep::Continue(d, next) => {
                    out.push(d);
                    s = next;
                }
                ScheduleStep::Done => break,
            }
        }
        out
    }

    #[test]
    fn stop_yields_done_immediately() {
        assert!(matches!(Schedule::stop().step(), ScheduleStep::Done));
    }

    #[test]
    fn once_yields_one_continue_then_done() {
        let delays = collect_delays(Schedule::once(), 5);
        assert_eq!(delays, vec![Duration::ZERO]);
    }

    #[test]
    fn recurs_yields_n_continues() {
        let delays = collect_delays(Schedule::recurs(3), 5);
        assert_eq!(delays, vec![Duration::ZERO, Duration::ZERO, Duration::ZERO]);
    }

    #[test]
    fn spaced_yields_fixed_delays() {
        let d = Duration::from_millis(10);
        let delays = collect_delays(Schedule::spaced(d), 3);
        assert_eq!(delays, vec![d, d, d]);
    }

    #[test]
    fn exponential_doubles_each_iteration() {
        let base = Duration::from_millis(10);
        let delays = collect_delays(Schedule::exponential(base), 4);
        assert_eq!(
            delays,
            vec![
                Duration::from_millis(10),
                Duration::from_millis(20),
                Duration::from_millis(40),
                Duration::from_millis(80),
            ]
        );
    }

    #[test]
    fn fibonacci_yields_fibonacci_delays() {
        let base = Duration::from_millis(10);
        let delays = collect_delays(Schedule::fibonacci(base), 5);
        assert_eq!(
            delays,
            vec![
                Duration::from_millis(10),
                Duration::from_millis(20),
                Duration::from_millis(30),
                Duration::from_millis(50),
                Duration::from_millis(80),
            ]
        );
    }

    #[test]
    fn max_attempts_caps_count() {
        let delays = collect_delays(Schedule::forever().max_attempts(3), 10);
        assert_eq!(delays.len(), 3);
    }

    #[test]
    fn bounded_stops_when_cumulative_exceeds_budget() {
        let s = Schedule::spaced(Duration::from_millis(40)).bounded(Duration::from_millis(100));
        let delays = collect_delays(s, 10);
        // 40 + 40 = 80 ≤ 100, third would be 120 > 100 → stops.
        assert_eq!(delays.len(), 2);
    }
}
