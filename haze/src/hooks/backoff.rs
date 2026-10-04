//! The wait between reconnection attempts used by the client hooks.

use std::time::Duration;

/// Wait times between reconnection attempts in the client hooks.
///
/// Starts at 1 second and doubles after each attempt, up to 30 seconds.
#[derive(Debug)]
pub(crate) struct Backoff {
    /// The wait before the next attempt.
    delay: Duration,
}

impl Backoff {
    /// The wait after the first drop, and after a connection delivered something.
    const FIRST: Duration = Duration::from_secs(1);
    /// The longest wait between two attempts.
    const LONGEST: Duration = Duration::from_secs(30);

    /// Starts at the shortest wait.
    pub(crate) const fn new() -> Self {
        Self { delay: Self::FIRST }
    }

    /// Returns the wait before the next attempt and doubles the following one.
    fn next(&mut self) -> Duration {
        let current = self.delay;
        self.delay = (self.delay * 2).min(Self::LONGEST);
        current
    }

    /// Goes back to the shortest wait, once a connection delivers something.
    pub(crate) const fn reset(&mut self) {
        self.delay = Self::FIRST;
    }

    /// Sleeps for the next wait: tokio's timer natively, the browser's timer on
    /// wasm, the same split `dioxus-sdk-time`'s `sleep` makes.
    pub(crate) async fn wait(&mut self) {
        let delay = self.next();
        #[cfg(not(target_family = "wasm"))]
        tokio::time::sleep(delay).await;
        #[cfg(target_family = "wasm")]
        gloo_timers::future::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Backoff;

    #[test]
    fn doubles_from_one_second_up_to_thirty() {
        let mut backoff = Backoff::new();
        let mut delays = Vec::new();
        for _ in 0..7 {
            delays.push(backoff.next().as_secs());
        }
        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn reset_starts_again_from_one_second() {
        let mut backoff = Backoff::new();
        backoff.next();
        backoff.next();
        backoff.reset();
        assert_eq!(backoff.next(), Duration::from_secs(1));
    }
}
