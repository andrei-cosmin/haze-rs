//! `use_websocket`: Dioxus's WebSocket hook with automatic reconnection.

use dioxus::{CapturedError, prelude::*};
use dioxus_fullstack::{
    Encoding, UseWebsocket, Websocket, WebsocketError, WebsocketState, serde::de::DeserializeOwned,
};

use crate::hooks::{backoff::Backoff, reconnect::Reconnect, timer::Idle};

/// Dioxus's [`use_websocket`](dioxus_fullstack::use_websocket) with automatic
/// reconnection.
///
/// `open` calls a server function that returns
/// [`Websocket<In, Out, Enc>`](dioxus_fullstack::Websocket), and `on_message` runs
/// for each message received. Returns Dioxus's [`UseWebsocket`] handle, so `send`
/// and `status` behave as in Dioxus.
///
/// - A message that fails to decode is skipped, and the connection stays open.
/// - When the socket closes or fails, a new one opens through
///   [`UseWebsocket::set`] after 1 second, doubling up to 30 seconds. The wait
///   resets once a message arrives. [`use_websocket_with`] adds an idle
///   timeout and jitter.
/// - Receive only through `on_message`. Calling `recv` on the handle as well
///   splits the messages between the two readers.
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
/// and receiving continues on the new one without counting as a drop.
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
    open: impl FnMut() -> Fut + 'static,
    on_message: impl FnMut(Out) + 'static,
) -> UseWebsocket<In, Out, Enc>
where
    In: 'static,
    Out: DeserializeOwned + 'static,
    Enc: Encoding + 'static,
    E: Into<CapturedError> + 'static,
    Fut: Future<Output = Result<Websocket<In, Out, Enc>, E>> + 'static,
{
    use_websocket_with(open, on_message, Reconnect::default())
}

/// [`use_websocket`] with [`Reconnect`] options: an idle timeout that drops and
/// reopens a socket that delivered no message for that long, and jitter on the
/// wait before each attempt. A message that fails to decode counts as
/// delivered.
///
/// `options` is read on the first render; later values are ignored. With
/// `Reconnect::default()` this is exactly [`use_websocket`].
///
/// # Examples
///
/// ```rust,ignore
/// let options = Reconnect { idle: Some(Duration::from_secs(45)), jitter: true };
/// let socket = haze::use_websocket_with(|| chat(WebSocketOptions::new()), move |message| log.push(message), options);
/// ```
pub fn use_websocket_with<In, Out, Enc, E, Fut>(
    mut open: impl FnMut() -> Fut + 'static,
    mut on_message: impl FnMut(Out) + 'static,
    options: Reconnect,
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
    let options = use_hook(|| options);
    use_future(move || async move {
        let mut backoff = Backoff::new(options.jitter);
        loop {
            if socket.connect().await == WebsocketState::Open {
                let mut idle = Idle::new(options.idle);
                while let Some(received) = idle.within(socket.recv()).await {
                    match received {
                        Ok(message) => {
                            backoff.reset();
                            on_message.call(message);
                            idle.touch();
                        }
                        Err(WebsocketError::Deserialization(_)) => idle.touch(),
                        Err(WebsocketError::ConnectionClosed { .. }) if !socket.is_closed() => {
                            idle.touch();
                        }
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

#[cfg(test)]
mod tests {
    #[cfg(feature = "server")]
    use std::time::Duration;

    use dioxus_fullstack::{JsonEncoding, ServerFnError};
    use tokio::time::Instant;

    use super::*;
    use crate::hooks::test_log::Log;
    #[cfg(feature = "server")]
    use crate::hooks::test_server::QuietServer;

    #[component]
    fn Refused(log: Log, options: Reconnect) -> Element {
        let opened = log.opened;
        use_websocket_with(
            move || {
                opened.borrow_mut().push(Instant::now());
                async {
                    Err::<Websocket<String, String, JsonEncoding>, _>(ServerFnError::new("refused"))
                }
            },
            |_| {},
            options,
        );
        rsx! {}
    }

    #[cfg(feature = "server")]
    #[component]
    fn QuietSocket(log: Log, server: QuietServer, options: Reconnect) -> Element {
        let opened = log.opened;
        let items = log.items;
        use_websocket_with(
            move || {
                opened.borrow_mut().push(Instant::now());
                server.open_socket()
            },
            move |item| items.borrow_mut().push(item),
            options,
        );
        rsx! {}
    }

    #[cfg(feature = "server")]
    #[component]
    fn SteadySocket(log: Log, server: QuietServer) -> Element {
        let opened = log.opened;
        let items = log.items;
        use_websocket_with(
            move || {
                opened.borrow_mut().push(Instant::now());
                server.open_steady_socket()
            },
            move |item| items.borrow_mut().push(item),
            Reconnect {
                idle: Some(QuietServer::STEADY_GAP * 3 / 2),
                jitter: false,
            },
        );
        rsx! {}
    }

    #[cfg(feature = "server")]
    #[component]
    fn Switching(log: Log, server: QuietServer) -> Element {
        let mut page = use_signal(|| 1_u32);
        let opened = log.opened;
        let items = log.items;
        use_websocket(
            move || {
                page.read();
                opened.borrow_mut().push(Instant::now());
                server.open_socket()
            },
            move |item| {
                items.borrow_mut().push(item);
                if items.borrow().len() == 1 {
                    page.set(2);
                }
            },
        );
        rsx! {}
    }

    #[cfg(feature = "server")]
    #[tokio::test]
    async fn messages_within_the_idle_timeout_keep_the_socket_open_past_one_that_fails_to_decode() {
        let log = Log::default();
        let props = SteadySocketProps {
            log: log.clone(),
            server: QuietServer::start().await,
        };
        let mut dom = VirtualDom::new_with_props(SteadySocket, props);
        Log::drive(&mut dom, || log.items().len() >= 3).await;
        assert_eq!(*log.items(), [1, 2, 3]);
        assert_eq!(log.opened.borrow().len(), 1);
    }

    #[cfg(feature = "server")]
    #[tokio::test]
    async fn a_socket_replaced_on_a_signal_change_keeps_receiving_without_a_reconnect() {
        let log = Log::default();
        let props = SwitchingProps {
            log: log.clone(),
            server: QuietServer::start().await,
        };
        let mut dom = VirtualDom::new_with_props(Switching, props);
        Log::drive(&mut dom, || log.items().len() >= 2).await;
        assert_eq!(*log.items(), [1, 1]);
        assert_eq!(log.opened.borrow().len(), 2);
    }

    #[cfg(feature = "server")]
    #[tokio::test]
    async fn a_quiet_socket_is_dropped_after_the_idle_timeout_and_reopened() {
        let log = Log::default();
        let props = QuietSocketProps {
            log: log.clone(),
            server: QuietServer::start().await,
            options: Reconnect {
                idle: Some(Duration::from_millis(300)),
                jitter: false,
            },
        };
        let mut dom = VirtualDom::new_with_props(QuietSocket, props);
        Log::drive(&mut dom, || log.items().len() >= 2).await;
        let gap = log.spacing()[0];
        assert!(gap >= Duration::from_millis(1300) && gap < Duration::from_millis(2500));
        assert_eq!(log.items()[..2], [1, 1]);
    }

    #[cfg(feature = "server")]
    #[tokio::test]
    async fn without_an_idle_timeout_a_quiet_socket_stays_open() {
        let log = Log::default();
        let props = QuietSocketProps {
            log: log.clone(),
            server: QuietServer::start().await,
            options: Reconnect::default(),
        };
        let mut dom = VirtualDom::new_with_props(QuietSocket, props);
        let deadline = Instant::now() + Duration::from_millis(1500);
        Log::drive(&mut dom, || Instant::now() >= deadline).await;
        assert_eq!(log.opened.borrow().len(), 1);
        assert_eq!(*log.items(), [1]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_socket_backs_off_one_two_then_four_seconds() {
        let log = Log::default();
        let props = RefusedProps {
            log: log.clone(),
            options: Reconnect::default(),
        };
        let mut dom = VirtualDom::new_with_props(Refused, props);
        Log::drive(&mut dom, || log.opened.borrow().len() >= 4).await;
        assert_eq!(log.gaps()[..3], [1, 2, 4]);
    }

    #[tokio::test(start_paused = true)]
    async fn with_jitter_two_socket_clients_wait_differently_within_half_and_all_of_each_wait() {
        let first = Log::default();
        let second = Log::default();
        for log in [&first, &second] {
            let props = RefusedProps {
                log: log.clone(),
                options: Reconnect {
                    idle: None,
                    jitter: true,
                },
            };
            let mut dom = VirtualDom::new_with_props(Refused, props);
            Log::drive(&mut dom, || log.opened.borrow().len() >= 4).await;
        }
        assert!(Log::within_jitter(&first.spacing(), &[1, 2, 4]));
        assert!(Log::within_jitter(&second.spacing(), &[1, 2, 4]));
        assert_ne!(first.spacing()[..3], second.spacing()[..3]);
    }
}
