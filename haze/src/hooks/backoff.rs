//! The wait between reconnection attempts used by the client hooks.

#[cfg(not(target_family = "wasm"))]
use std::hash::{BuildHasher, RandomState};
use std::time::Duration;

use crate::hooks::timer::Timer;

/// Wait times between reconnection attempts in the client hooks.
///
/// Starts at 1 second and doubles after each attempt, up to 30 seconds. With
/// jitter, each wait is drawn uniformly between half of it and all of it.
#[derive(Debug)]
pub(crate) struct Backoff {
    /// The wait before the next attempt.
    delay: Duration,
    /// Whether each wait is randomised.
    jitter: bool,
}

impl Backoff {
    /// The wait after the first drop, and after a connection delivered something.
    const FIRST: Duration = Duration::from_secs(1);
    /// The longest wait between two attempts.
    const LONGEST: Duration = Duration::from_secs(30);

    /// Starts at the shortest wait, randomising each wait when `jitter` is set.
    pub(crate) const fn new(jitter: bool) -> Self {
        Self {
            delay: Self::FIRST,
            jitter,
        }
    }

    /// Returns the wait before the next attempt and doubles the following one.
    fn next(&mut self) -> Duration {
        let current = self.delay;
        self.delay = (self.delay * 2).min(Self::LONGEST);
        if self.jitter {
            Self::spread(current, Self::fraction())
        } else {
            current
        }
    }

    /// The point `fraction` of the way from half of `delay` to all of it.
    fn spread(delay: Duration, fraction: f64) -> Duration {
        let half = delay / 2;
        half + half.mul_f64(fraction)
    }

    /// A random number in `[0, 1)`: from a freshly keyed `RandomState`
    /// natively, whose keys are random per process and differ per call, and
    /// from `Math.random` in the browser.
    #[cfg(not(target_family = "wasm"))]
    fn fraction() -> f64 {
        let bits = RandomState::new().hash_one(0_u8) >> 32;
        let high = u32::try_from(bits).unwrap_or(u32::MAX);
        f64::from(high) / (f64::from(u32::MAX) + 1.0)
    }

    /// A random number in `[0, 1)` from `Math.random`.
    #[cfg(target_family = "wasm")]
    fn fraction() -> f64 {
        js_sys::Math::random()
    }

    /// Goes back to the shortest wait, once a connection delivers something.
    pub(crate) const fn reset(&mut self) {
        self.delay = Self::FIRST;
    }

    /// Sleeps for the next wait.
    pub(crate) async fn wait(&mut self) {
        Timer::sleep(self.next()).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Backoff;

    #[test]
    fn doubles_from_one_second_up_to_thirty() {
        let mut backoff = Backoff::new(false);
        let mut delays = Vec::new();
        for _ in 0..7 {
            delays.push(backoff.next().as_secs());
        }
        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn reset_starts_again_from_one_second() {
        let mut backoff = Backoff::new(false);
        backoff.next();
        backoff.next();
        backoff.reset();
        assert_eq!(backoff.next(), Duration::from_secs(1));
    }

    #[test]
    fn a_spread_wait_lies_between_half_and_all_of_it() {
        let delay = Duration::from_secs(8);
        assert_eq!(Backoff::spread(delay, 0.0), Duration::from_secs(4));
        assert_eq!(Backoff::spread(delay, 0.5), Duration::from_secs(6));
        assert!(Backoff::spread(delay, 0.999_999) < delay);
    }

    #[test]
    fn jittered_waits_stay_within_their_bounds_and_still_double() {
        let mut backoff = Backoff::new(true);
        for full in [1, 2, 4, 8, 16, 30, 30] {
            let wait = backoff.next();
            assert!(wait >= Duration::from_secs(full) / 2);
            assert!(wait < Duration::from_secs(full));
        }
    }

    #[test]
    fn two_jittered_backoffs_draw_different_waits() {
        let mut first = Backoff::new(true);
        let mut second = Backoff::new(true);
        let mut firsts = Vec::new();
        let mut seconds = Vec::new();
        for _ in 0..4 {
            firsts.push(first.next());
            seconds.push(second.next());
        }
        assert_ne!(firsts, seconds);
    }
}
