//! The connection state reported by `use_streaming` and `use_server_events`.

/// The state of a live connection opened by [`use_streaming`](crate::use_streaming)
/// or [`use_server_events`](crate::use_server_events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// Opening the connection.
    Connecting,
    /// Connected and receiving.
    Open,
    /// The connection dropped or could not be opened; waiting before the next attempt.
    Retrying,
}
