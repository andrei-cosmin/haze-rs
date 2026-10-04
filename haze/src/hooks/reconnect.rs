//! The options of the reconnecting hooks.

use std::time::Duration;

/// How [`use_streaming_with`](crate::use_streaming_with),
/// [`use_server_events_with`](crate::use_server_events_with) and
/// [`use_websocket_with`](crate::use_websocket_with) watch a connection and
/// wait before opening the next one.
///
/// The default reproduces [`use_streaming`](crate::use_streaming),
/// [`use_server_events`](crate::use_server_events) and
/// [`use_websocket`](crate::use_websocket): no idle timeout and no jitter, so
/// the waits are exactly 1, 2, 4 and 8 seconds, doubling up to 30.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
///
/// use haze::Reconnect;
///
/// let options = Reconnect {
///     idle: Some(Duration::from_secs(45)),
///     jitter: true,
/// };
/// assert_ne!(options, Reconnect::default());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconnect {
    /// Drops and reopens an open connection that delivered nothing for this
    /// long, so a half-open connection is noticed without waiting for TCP; the
    /// server must send data often enough, such as a ping, to stay under it.
    /// `None` keeps a quiet connection open.
    pub idle: Option<Duration>,
    /// Randomises each wait between half of it and all of it, so clients that
    /// lost one server do not all reconnect at the same moment.
    pub jitter: bool,
}
