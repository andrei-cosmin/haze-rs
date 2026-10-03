//! Client hooks that keep a streaming, server-sent events, or WebSocket server function connected.

mod backoff;
mod connection;
mod server_events;
mod streaming;
mod websocket;

pub use connection::Connection;
pub use server_events::use_server_events;
pub use streaming::use_streaming;
pub use websocket::use_websocket;
