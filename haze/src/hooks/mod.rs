//! Client hooks that keep a streaming, server-sent events, or WebSocket server function connected.

mod backoff;
mod connection;
mod reconnect;
mod server_events;
mod streaming;
#[cfg(test)]
mod test_log;
#[cfg(all(test, feature = "server"))]
mod test_server;
mod timer;
mod websocket;

pub use connection::Connection;
pub use reconnect::Reconnect;
pub use server_events::{use_server_events, use_server_events_with};
pub use streaming::{use_streaming, use_streaming_with};
pub use websocket::{use_websocket, use_websocket_with};
