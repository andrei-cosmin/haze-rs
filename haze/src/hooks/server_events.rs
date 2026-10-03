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
/// Signals read while `open` builds its request, such as `page()` in
/// `move || watch(page())`, are tracked through [`use_effect`]: when one changes,
/// the current stream is dropped and a new one opens. Signals read inside the
/// future `open` returns are not tracked, unlike in [`use_resource`] and
/// [`use_websocket`](crate::use_websocket), so read them before the `async`
/// block, as in `move || { let page = page(); async move { watch(page).await } }`;
/// signals read inside `on_event` are not tracked either. The receive task does
/// not run during server-side rendering, keeps running while the component
/// returns early before reaching this hook, the way [`use_resource`] keeps its
/// future running, and is canceled when the component is dropped. Opening and
/// stream errors, and the end of the stream, trigger reconnection; errors are
/// not returned to the caller.
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
