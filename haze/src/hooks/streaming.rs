//! `use_streaming`: receives a streaming server function and reconnects.

use std::{cell::Cell, rc::Rc};

use dioxus::{core::Task, prelude::*};
use dioxus_fullstack::Streaming;

use crate::hooks::{backoff::Backoff, connection::Connection};

/// Receives every value from a streaming server function, reconnecting when the
/// stream ends or fails.
///
/// `open` calls a server function that returns
/// [`Streaming<T, Enc>`](dioxus_fullstack::Streaming); `on_item` runs for each
/// value received. A decoding error ends the stream like any other error, as
/// Dioxus's frame decoder cannot resume after one. After a drop, the next
/// attempt waits 1 second, doubling up to 30 seconds, and the wait resets once
/// an item arrives. Returns the current [`Connection`] state.
///
/// # Lifecycle
///
/// Signals read while `open` builds its request, such as `page()` in
/// `move || watch(page())`, are tracked through [`use_effect`]: when one changes,
/// the current stream is dropped and a new one opens. Signals read inside the
/// future `open` returns are not tracked, unlike in [`use_resource`] and
/// [`use_websocket`](crate::use_websocket), so read them before the `async`
/// block, as in `move || { let page = page(); async move { watch(page).await } }`;
/// signals read inside `on_item` are not tracked either. The receive task does
/// not run during server-side rendering, keeps running while the component
/// returns early before reaching this hook, the way [`use_resource`] keeps its
/// future running, and is canceled when the component is dropped. Opening and
/// stream errors, and the end of the stream, trigger reconnection; errors are
/// not returned to the caller.
///
/// # Examples
///
/// ```rust,ignore
/// let mut count = use_signal(|| 0);
/// let status = haze::use_streaming(count_stream, move |value| count.set(value));
/// ```
#[track_caller]
pub fn use_streaming<T, Enc, E, Fut>(
    mut open: impl FnMut() -> Fut + 'static,
    mut on_item: impl FnMut(T) + 'static,
) -> ReadSignal<Connection>
where
    T: Send + 'static,
    Enc: 'static,
    E: 'static,
    Fut: Future<Output = Result<Streaming<T, Enc>, E>> + 'static,
{
    let mut status = use_signal(|| Connection::Connecting);
    let open = use_callback(move |()| open());
    let on_item = use_callback(move |item: T| on_item(item));
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
                if let Ok(mut stream) = opening.await {
                    status.set(Connection::Open);
                    while let Some(Ok(item)) = stream.next().await {
                        backoff.reset();
                        on_item.call(item);
                    }
                }
                status.set(Connection::Retrying);
                backoff.wait().await;
            }
        })));
    });
    use_hook(|| ReadSignal::new(status))
}
