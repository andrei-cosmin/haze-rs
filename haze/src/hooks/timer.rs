//! The timers of the client hooks: tokio's natively, the browser's on wasm,
//! the same split `dioxus-sdk-time`'s `sleep` makes.

use std::{
    future::{Future, poll_fn},
    pin::{Pin, pin},
    task::{Context, Poll},
    time::Duration,
};

#[cfg(not(target_family = "wasm"))]
use tokio::time::Instant;
#[cfg(target_family = "wasm")]
use web_time::Instant;

/// Sleeps.
#[derive(Debug)]
pub(crate) struct Timer;

impl Timer {
    /// Sleeps for `duration`.
    pub(crate) async fn sleep(duration: Duration) {
        #[cfg(not(target_family = "wasm"))]
        tokio::time::sleep(duration).await;
        #[cfg(target_family = "wasm")]
        gloo_timers::future::sleep(duration).await;
    }
}

pub(crate) struct Idle {
    window: Option<Window>,
}

impl Idle {
    pub(crate) fn new(limit: Option<Duration>) -> Self {
        Self {
            window: limit.map(Window::new),
        }
    }

    pub(crate) fn touch(&mut self) {
        if let Some(window) = &mut self.window {
            window.last = Instant::now();
        }
    }

    pub(crate) async fn within<F: Future>(&mut self, future: F) -> Option<F::Output> {
        let mut future = pin!(future);
        poll_fn(|context| {
            if let Poll::Ready(output) = future.as_mut().poll(context) {
                return Poll::Ready(Some(output));
            }
            self.window.as_mut().map_or(Poll::Pending, |window| {
                window.poll_expired(context).map(|()| None)
            })
        })
        .await
    }
}

struct Window {
    limit: Duration,
    last: Instant,
    alarm: Pin<Box<dyn Future<Output = ()>>>,
}

impl Window {
    fn new(limit: Duration) -> Self {
        Self {
            limit,
            last: Instant::now(),
            alarm: Box::pin(Timer::sleep(limit)),
        }
    }

    fn poll_expired(&mut self, context: &mut Context<'_>) -> Poll<()> {
        while self.alarm.as_mut().poll(context).is_ready() {
            let left = self.limit.saturating_sub(self.last.elapsed());
            if left.is_zero() {
                return Poll::Ready(());
            }
            self.alarm = Box::pin(Timer::sleep(left));
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, time::Duration};

    use tokio::time::Instant;

    use super::{Idle, Timer};

    #[tokio::test(start_paused = true)]
    async fn a_future_that_finishes_in_time_returns_its_output() {
        let mut idle = Idle::new(Some(Duration::from_secs(5)));
        let output = idle
            .within(async {
                Timer::sleep(Duration::from_secs(2)).await;
                7
            })
            .await;
        assert_eq!(output, Some(7));
    }

    #[tokio::test(start_paused = true)]
    async fn a_future_still_running_at_the_limit_returns_none() {
        let start = Instant::now();
        let mut idle = Idle::new(Some(Duration::from_secs(5)));
        assert_eq!(idle.within(pending::<u8>()).await, None);
        assert_eq!(start.elapsed(), Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_limit_the_future_runs_to_its_output() {
        let mut idle = Idle::new(None);
        let output = idle
            .within(async {
                Timer::sleep(Duration::from_secs(3600)).await;
                9
            })
            .await;
        assert_eq!(output, Some(9));
    }

    #[tokio::test(start_paused = true)]
    async fn activity_restarts_the_limit_from_the_last_touch() {
        let start = Instant::now();
        let mut idle = Idle::new(Some(Duration::from_secs(5)));
        let first = idle
            .within(async {
                Timer::sleep(Duration::from_secs(3)).await;
                1
            })
            .await;
        idle.touch();
        assert_eq!(first, Some(1));
        assert_eq!(idle.within(pending::<u8>()).await, None);
        assert_eq!(start.elapsed(), Duration::from_secs(8));
    }
}
