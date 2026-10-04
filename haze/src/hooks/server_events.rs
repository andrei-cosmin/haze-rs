//! `use_server_events`: receives server-sent events and reconnects.

use std::{cell::Cell, rc::Rc};

use dioxus::{core::Task, prelude::*};
use dioxus_fullstack::{ServerEvents, ServerFnError, serde::de::DeserializeOwned};

use crate::hooks::{backoff::Backoff, connection::Connection};

/// Receives every event from a server-sent events server function, reconnecting
/// when the stream ends or fails.
///
/// `open` calls a server function that returns
/// [`ServerEvents<T>`](dioxus_fullstack::ServerEvents); `on_event` runs for each
/// event received; an event that fails to decode is skipped without dropping
/// the connection. Reconnects with the same backoff as
/// [`use_streaming`](crate::use_streaming).
/// Returns the current [`Connection`] state.
///
/// This hook needs a fullstack build, since only `dioxus-fullstack`'s
/// `server` feature can build a `ServerEvents` response; a server function
/// that returns [`Streaming`](dioxus_fullstack::Streaming), received with
/// [`use_streaming`](crate::use_streaming), works in every build, `standalone`
/// included.
///
/// # Lifecycle
///
/// - Signals read while `open` builds its request, such as `page()` in
///   `move || watch(page())`, are tracked through [`use_effect`]. When one
///   changes, the current stream is dropped and a new one opens.
/// - Signals read inside the future that `open` returns are not tracked,
///   unlike in [`use_resource`] and [`use_websocket`](crate::use_websocket).
///   Read them before the `async` block:
///   `move || { let page = page(); async move { watch(page).await } }`.
/// - Signals read inside `on_event` are not tracked.
/// - The receive task does not run during server-side rendering. It keeps
///   running while the component returns early, as [`use_resource`] does,
///   and is canceled when the component is dropped.
/// - Opening errors, stream errors and the end of the stream trigger a
///   reconnect. Errors are not returned to the caller.
///
/// # Examples
///
/// ```rust,ignore
/// let mut toasts = use_signal(Vec::new);
/// let status = haze::use_server_events(notifications, move |toast| toasts.push(toast));
/// ```
#[track_caller]
pub fn use_server_events<T, E, Fut>(
    mut open: impl FnMut() -> Fut + 'static,
    mut on_event: impl FnMut(T) + 'static,
) -> ReadSignal<Connection>
where
    T: DeserializeOwned + 'static,
    E: 'static,
    Fut: Future<Output = Result<ServerEvents<T>, E>> + 'static,
{
    let mut status = use_signal(|| Connection::Connecting);
    let open = use_callback(move |()| open());
    let on_event = use_callback(move |event: T| on_event(event));
    let task = use_hook(|| Rc::new(Cell::new(None::<Task>)));
    use_effect(move || {
        let mut first = Some(open.call(()));
        if let Some(previous) = task.take() {
            previous.cancel();
        }
        task.set(Some(spawn(async move {
            let mut backoff = Backoff::new();
            loop {
                status.set(Connection::Connecting);
                let opening = first.take().unwrap_or_else(|| open.call(()));
                if let Ok(mut events) = opening.await {
                    status.set(Connection::Open);
                    loop {
                        match events.recv().await {
                            Some(Ok(event)) => {
                                backoff.reset();
                                on_event.call(event);
                            }
                            Some(Err(ServerFnError::Serialization(_))) => {}
                            Some(Err(_)) | None => break,
                        }
                    }
                }
                status.set(Connection::Retrying);
                backoff.wait().await;
            }
        })));
    });
    use_hook(|| ReadSignal::new(status))
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use std::{convert::Infallible, time::Duration};

    use tokio::time::Instant;

    use super::*;
    use crate::hooks::test_log::Log;

    #[component]
    fn Announcements(log: Log) -> Element {
        let opened = log.opened.clone();
        let status = use_server_events(
            move || {
                opened.borrow_mut().push(Instant::now());
                async { Ok::<_, Infallible>(ServerEvents::<u32>::new(|_| async {})) }
            },
            |_| {},
        );
        log.watch(status);
        rsx! {}
    }

    #[component]
    fn HiddenAnnouncements(log: Log) -> Element {
        let mut hidden = use_signal(|| false);
        if hidden() {
            return rsx! {};
        }
        let opened = log.opened.clone();
        use_server_events(
            move || {
                opened.borrow_mut().push(Instant::now());
                hidden.set(true);
                async { Ok::<_, Infallible>(ServerEvents::<u32>::new(|_| async {})) }
            },
            |_| {},
        );
        rsx! {}
    }

    #[tokio::test(start_paused = true)]
    async fn use_server_events_reconnects_with_the_same_backoff() {
        let log = Log::default();
        let mut dom =
            VirtualDom::new_with_props(Announcements, AnnouncementsProps { log: log.clone() });
        Log::drive(&mut dom, || log.opened.borrow().len() >= 3).await;
        assert_eq!(log.gaps()[..2], [1, 2]);
        assert!(log.states.borrow().contains(&Connection::Retrying));
        assert!(log.same_handle_every_render());
    }

    #[tokio::test(start_paused = true)]
    async fn use_server_events_keeps_reconnecting_while_its_component_returns_early() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(
            HiddenAnnouncements,
            HiddenAnnouncementsProps { log: log.clone() },
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        Log::drive(&mut dom, || Instant::now() >= deadline).await;
        assert!(log.opened.borrow().len() >= 3);
        assert_eq!(log.gaps()[..2], [1, 2]);
    }
}
