//! `use_websocket`: Dioxus's WebSocket hook with automatic reconnection.

use dioxus::{CapturedError, prelude::*};
use dioxus_fullstack::{
    Encoding, UseWebsocket, Websocket, WebsocketError, WebsocketState, serde::de::DeserializeOwned,
};

use crate::hooks::backoff::Backoff;

/// Dioxus's [`use_websocket`](dioxus_fullstack::use_websocket) with automatic
/// reconnection.
///
/// `open` calls a server function that returns
/// [`Websocket<In, Out, Enc>`](dioxus_fullstack::Websocket); `on_message` runs for
/// each message received; a message that fails to decode is skipped without
/// dropping the connection. When the socket closes or fails, a new one is opened
/// with [`UseWebsocket::set`] after 1 second, doubling up to 30 seconds, and the
/// wait resets once a message arrives. Returns Dioxus's [`UseWebsocket`] handle,
/// so `send` and `status` behave as in Dioxus. Receive only through
/// `on_message`: calling `recv` on the handle as well would split the messages
/// between the two readers.
///
/// This hook needs a fullstack build, since only `dioxus-fullstack`'s
/// `server` feature can upgrade a connection; in a `standalone` build, push
/// to the client with a server function that returns
/// [`Streaming`](dioxus_fullstack::Streaming), received with
/// [`use_streaming`](crate::use_streaming), and send commands as ordinary
/// [`#[haze::server]`](macro@crate::server) calls.
///
/// # Lifecycle
///
/// Signals read while `open` runs are tracked the way Dioxus's own
/// `use_websocket` tracks them: when one changes, Dioxus replaces the socket,
/// and receiving simply continues on the new one without counting as a drop.
/// The receive task does not run during server-side rendering, is paused while
/// the component returns early before reaching this hook, as [`use_future`]
/// pauses its task, and is canceled when the component is dropped. Opening and
/// stream errors trigger reconnection; they are not returned to the caller.
///
/// This hook has the same name as Dioxus's; call it as `haze::use_websocket`.
///
/// # Examples
///
/// ```rust,ignore
/// let socket = haze::use_websocket(|| chat(WebSocketOptions::new()), move |message| log.push(message));
/// socket.send(ClientMessage::Typing).await?;
/// ```
pub fn use_websocket<In, Out, Enc, E, Fut>(
    mut open: impl FnMut() -> Fut + 'static,
    mut on_message: impl FnMut(Out) + 'static,
) -> UseWebsocket<In, Out, Enc>
where
    In: 'static,
    Out: DeserializeOwned + 'static,
    Enc: Encoding + 'static,
    E: Into<CapturedError> + 'static,
    Fut: Future<Output = Result<Websocket<In, Out, Enc>, E>> + 'static,
{
    let open = use_callback(move |()| open());
    let mut socket = dioxus_fullstack::use_websocket(move || open.call(()));
    let on_message = use_callback(move |message: Out| on_message(message));
    use_future(move || async move {
        let mut backoff = Backoff::new();
        loop {
            if socket.connect().await == WebsocketState::Open {
                loop {
                    match socket.recv().await {
                        Ok(message) => {
                            backoff.reset();
                            on_message.call(message);
                        }
                        Err(WebsocketError::Deserialization(_)) => {}
                        Err(WebsocketError::ConnectionClosed { .. }) if !socket.is_closed() => {}
                        Err(_) => break,
                    }
                }
            }
            backoff.wait().await;
            socket.set(open.call(()).await);
        }
    });
    socket
}
