use std::time::{Duration, SystemTime};

/// Defines when a scheduled task should run.
#[derive(Debug, Clone)]
pub enum Schedule {
    /// Run at a fixed interval from the last run.
    Interval(Duration),
    /// Run according to a cron expression (e.g. `"0 0 * * * *"` for every hour).
    ///
    /// The expression is parsed lazily by [`next_after`](Schedule::next_after).
    Cron(String),
    /// Run exactly once at the given time.
    Once(SystemTime),
    /// Run repeatedly starting at `start`, then every `interval` thereafter.
    Recurring {
        /// First run time.
        start: SystemTime,
        /// Interval between subsequent runs.
        interval: Duration,
    },
}

impl Schedule {
    /// Compute the next run time after `from`.
    ///
    /// Returns `None` if no future run exists (e.g. a past `Once` schedule).
    pub fn next_after(&self, from: SystemTime) -> Option<SystemTime> {
        match self {
            Self::Interval(d) => from.checked_add(*d),
            Self::Cron(expr) => {
                let s: cron::Schedule = expr.parse().ok()?;
                let from_dt: chrono::DateTime<chrono::Utc> = from.into();
                s.after(&from_dt)
                    .next()
                    .map(|dt| -> SystemTime { dt.into() })
            }
            Self::Once(t) => {
                if from < *t {
                    Some(*t)
                } else {
                    None
                }
            }
            Self::Recurring { start, interval } => {
                if from < *start {
                    return Some(*start);
                }
                let elapsed = from.duration_since(*start).ok()?;
                if elapsed.is_zero() || interval.is_zero() {
                    return None;
                }
                // Use seconds-based arithmetic when possible to avoid
                // u128 overflow for long-running recurring schedules.
                let elapsed_secs = elapsed.as_secs();
                let interval_secs = interval.as_secs();
                if interval_secs > 0 {
                    let intervals_passed = elapsed_secs.checked_div(interval_secs)?;
                    let next = intervals_passed.checked_add(1)?;
                    let dur = Duration::from_secs(interval_secs.checked_mul(next)?);
                    start.checked_add(dur)
                } else {
                    // Sub-second interval — use nanos (values are small so u64 is safe)
                    let elapsed_ns: u64 = elapsed.as_nanos().try_into().ok()?;
                    let interval_ns: u64 = interval.as_nanos().try_into().ok()?;
                    let intervals_passed = elapsed_ns.checked_div(interval_ns)?;
                    let next = intervals_passed.checked_add(1)?;
                    let dur = Duration::from_nanos(interval_ns.checked_mul(next)?);
                    start.checked_add(dur)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn test_interval_next() {
        let now = SystemTime::now();
        let sched = Schedule::Interval(Duration::from_secs(60));
        let next = sched.next_after(now).unwrap();
        assert!(next >= now + Duration::from_secs(60) - Duration::from_millis(1));
        assert!(next <= now + Duration::from_secs(60) + Duration::from_millis(1));
    }

    #[test]
    fn test_once_future() {
        let now = SystemTime::now();
        let future = now + Duration::from_secs(3600);
        let sched = Schedule::Once(future);
        assert_eq!(sched.next_after(now), Some(future));
    }

    #[test]
    fn test_once_past() {
        let now = SystemTime::now();
        let past = now - Duration::from_secs(3600);
        let sched = Schedule::Once(past);
        assert_eq!(sched.next_after(now), None);
    }

    #[test]
    fn test_recurring_before_start() {
        let now = SystemTime::now();
        let start = now + Duration::from_secs(300);
        let sched = Schedule::Recurring {
            start,
            interval: Duration::from_secs(60),
        };
        assert_eq!(sched.next_after(now), Some(start));
    }

    #[test]
    fn test_recurring_after_start() {
        let start = SystemTime::now() - Duration::from_secs(500);
        let sched = Schedule::Recurring {
            start,
            interval: Duration::from_secs(60),
        };
        let now = SystemTime::now();
        let next = sched.next_after(now).unwrap();
        // Should be at most 60 seconds from now
        let diff = next.duration_since(now).unwrap_or_default();
        assert!(diff <= Duration::from_secs(60));
    }

    #[test]
    fn test_cron_every_minute() {
        let sched = Schedule::Cron("0 * * * * *".into());
        let now = SystemTime::now();
        let next = sched.next_after(now);
        assert!(next.is_some());
        let next = next.unwrap();
        assert!(next > now);
    }

    #[test]
    fn test_cron_invalid_expression() {
        let sched = Schedule::Cron("not-a-cron".into());
        let now = SystemTime::now();
        assert_eq!(sched.next_after(now), None);
    }
}
