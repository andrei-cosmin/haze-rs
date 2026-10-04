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
/// - Signals read while `open` builds its request, such as `page()` in
///   `move || watch(page())`, are tracked through [`use_effect`]. When one
///   changes, the current stream is dropped and a new one opens.
/// - Signals read inside the future that `open` returns are not tracked,
///   unlike in [`use_resource`] and [`use_websocket`](crate::use_websocket).
///   Read them before the `async` block:
///   `move || { let page = page(); async move { watch(page).await } }`.
/// - Signals read inside `on_item` are not tracked.
/// - The receive task does not run during server-side rendering. It keeps
///   running while the component returns early, as [`use_resource`] does,
///   and is canceled when the component is dropped.
/// - Opening errors, stream errors and the end of the stream trigger a
///   reconnect. Errors are not returned to the caller.
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

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, time::Duration};

    use dioxus::fullstack::{
        Encoding, JsonEncoding,
        body::Body,
        encode_stream_frame,
        extract::{FromRequest, Request},
        http::header::CONTENT_TYPE,
    };
    use futures_util::{StreamExt, stream};
    use tokio::time::Instant;

    use super::*;
    use crate::hooks::test_log::Log;

    #[component]
    fn Counter(log: Log) -> Element {
        let opened = log.opened.clone();
        let items = log.items.clone();
        let status = use_streaming(
            move || {
                opened.borrow_mut().push(Instant::now());
                async { Ok::<_, Infallible>(Streaming::<u32, ()>::new(stream::iter([1, 2]))) }
            },
            move |item| items.borrow_mut().push(item),
        );
        log.watch(status);
        rsx! {}
    }

    #[component]
    fn Holding(log: Log) -> Element {
        let items = log.items.clone();
        let status = use_streaming(
            || async {
                let items = stream::iter([1]).chain(stream::pending());
                Ok::<_, Infallible>(Streaming::<u32, ()>::new(items))
            },
            move |item| items.borrow_mut().push(item),
        );
        log.record(status());
        rsx! {}
    }

    #[component]
    fn Silent(log: Log) -> Element {
        let opened = log.opened.clone();
        let status = use_streaming(
            move || {
                opened.borrow_mut().push(Instant::now());
                async { Ok::<_, Infallible>(Streaming::<u32, ()>::new(stream::empty())) }
            },
            |_| {},
        );
        log.record(status());
        rsx! {}
    }

    #[component]
    fn Refused(log: Log) -> Element {
        let opened = log.opened.clone();
        let status = use_streaming(
            move || {
                opened.borrow_mut().push(Instant::now());
                async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Err::<Streaming<u32, ()>, _>(())
                }
            },
            |_| {},
        );
        log.record(status());
        rsx! {}
    }

    #[component]
    fn Pager(log: Log) -> Element {
        let mut page = use_signal(|| 1_u32);
        let items = log.items.clone();
        use_streaming(
            move || {
                let page = page();
                async move {
                    let next = stream::once(async move {
                        tokio::time::sleep(Duration::from_secs(u64::from(page))).await;
                        page * 10 + 1
                    });
                    let items = stream::iter([page * 10])
                        .chain(next)
                        .chain(stream::pending());
                    Ok::<_, Infallible>(Streaming::<u32, ()>::new(items))
                }
            },
            move |item| {
                items.borrow_mut().push(item);
                if item == 10 {
                    page.set(2);
                }
            },
        );
        rsx! {}
    }

    #[component]
    fn Tally(log: Log) -> Element {
        let mut total = use_signal(|| 0_u32);
        let opened = log.opened.clone();
        let items = log.items.clone();
        use_streaming(
            move || {
                opened.borrow_mut().push(Instant::now());
                async { Ok::<_, Infallible>(Streaming::<u32, ()>::new(Log::one_now_two_later())) }
            },
            move |item| {
                if item == 2 {
                    total.set(total() + item);
                }
                items.borrow_mut().push(item);
            },
        );
        rsx! {}
    }

    #[component]
    fn Gate(log: Log) -> Element {
        let shown = use_signal(|| true);
        rsx! {
            if shown() {
                Ticker { log, shown }
            }
        }
    }

    #[component]
    fn Ticker(log: Log, mut shown: Signal<bool>) -> Element {
        let opened = log.opened.clone();
        let items = log.items.clone();
        use_streaming(
            move || {
                opened.borrow_mut().push(Instant::now());
                async { Ok::<_, Infallible>(Streaming::<u32, ()>::new(Log::one_now_two_later())) }
            },
            move |item| {
                items.borrow_mut().push(item);
                shown.set(false);
            },
        );
        rsx! {}
    }

    #[component]
    fn Cut(log: Log) -> Element {
        let opened = log.opened.clone();
        let items = log.items.clone();
        let status = use_streaming(
            move || {
                opened.borrow_mut().push(Instant::now());
                let seven = encode_stream_frame::<u32, JsonEncoding>(7).unwrap();
                let eight = encode_stream_frame::<u32, JsonEncoding>(8).unwrap();
                let chunks = [seven, eight.slice(..2), eight.slice(2..)].map(Ok::<_, Infallible>);
                let request = Request::builder()
                    .header(CONTENT_TYPE, JsonEncoding::stream_content_type())
                    .body(Body::from_stream(stream::iter(chunks)))
                    .unwrap();
                Streaming::<u32, JsonEncoding>::from_request(request, &())
            },
            move |item| items.borrow_mut().push(item),
        );
        log.record(status());
        rsx! {}
    }

    #[component]
    fn Hiding(log: Log) -> Element {
        let mut hidden = use_signal(|| false);
        if hidden() {
            return rsx! {};
        }
        let items = log.items.clone();
        use_streaming(
            || async { Ok::<_, Infallible>(Streaming::<u32, ()>::new(Log::one_now_two_later())) },
            move |item| {
                items.borrow_mut().push(item);
                hidden.set(true);
            },
        );
        rsx! {}
    }

    #[tokio::test(start_paused = true)]
    async fn use_streaming_receives_every_item_and_reopens_after_the_stream_ends() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Counter, CounterProps { log: log.clone() });
        Log::drive(&mut dom, || log.items().len() >= 4).await;
        assert_eq!(log.items()[..4], [1, 2, 1, 2]);
    }

    #[tokio::test(start_paused = true)]
    async fn use_streaming_returns_the_same_handle_on_every_render() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Counter, CounterProps { log: log.clone() });
        Log::drive(&mut dom, || log.opened.borrow().len() >= 3).await;
        assert!(log.same_handle_every_render());
    }

    #[tokio::test(start_paused = true)]
    async fn use_streaming_reports_connecting_then_open_while_the_stream_lasts() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Holding, HoldingProps { log: log.clone() });
        Log::drive(&mut dom, || log.states.borrow().contains(&Connection::Open)).await;
        assert_eq!(log.items(), [1]);
        assert_eq!(
            *log.states.borrow(),
            [Connection::Connecting, Connection::Open]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn use_streaming_reports_retrying_while_it_waits() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Silent, SilentProps { log: log.clone() });
        Log::drive(&mut dom, || log.opened.borrow().len() >= 2).await;
        assert_eq!(log.states.borrow().first(), Some(&Connection::Connecting));
        assert!(log.states.borrow().contains(&Connection::Retrying));
    }

    #[tokio::test(start_paused = true)]
    async fn an_empty_stream_backs_off_one_two_then_four_seconds() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Silent, SilentProps { log: log.clone() });
        Log::drive(&mut dom, || log.opened.borrow().len() >= 4).await;
        assert_eq!(log.gaps()[..3], [1, 2, 4]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_open_backs_off_without_reporting_open() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Refused, RefusedProps { log: log.clone() });
        Log::drive(&mut dom, || log.opened.borrow().len() >= 4).await;
        assert_eq!(log.gaps()[..3], [1, 2, 4]);
        assert_eq!(log.states.borrow().first(), Some(&Connection::Connecting));
        assert!(log.states.borrow().contains(&Connection::Retrying));
        assert!(!log.states.borrow().contains(&Connection::Open));
    }

    #[tokio::test(start_paused = true)]
    async fn a_delivered_item_resets_the_backoff() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Counter, CounterProps { log: log.clone() });
        Log::drive(&mut dom, || log.opened.borrow().len() >= 4).await;
        assert_eq!(log.gaps()[..3], [1, 1, 1]);
    }

    #[tokio::test(start_paused = true)]
    async fn use_streaming_reopens_when_a_signal_read_by_open_changes_and_drops_the_old_stream() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Pager, PagerProps { log: log.clone() });
        Log::drive(&mut dom, || log.items().contains(&21)).await;
        assert_eq!(log.items(), [10, 20, 21]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_signal_read_inside_on_item_does_not_reopen_the_stream() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Tally, TallyProps { log: log.clone() });
        Log::drive(&mut dom, || log.items().contains(&2)).await;
        assert_eq!(log.items(), [1, 2]);
        assert_eq!(log.opened.borrow().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn use_streaming_stops_receiving_when_its_component_is_dropped() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Gate, GateProps { log: log.clone() });
        let deadline = Instant::now() + Duration::from_secs(3);
        Log::drive(&mut dom, || Instant::now() >= deadline).await;
        assert_eq!(log.items(), [1]);
        assert_eq!(log.opened.borrow().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn use_streaming_keeps_receiving_while_its_component_returns_early() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Hiding, HidingProps { log: log.clone() });
        let deadline = Instant::now() + Duration::from_secs(3);
        Log::drive(&mut dom, || Instant::now() >= deadline).await;
        assert_eq!(log.items(), [1, 2]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_frame_cut_across_chunks_drops_the_stream_and_reopens_it() {
        let log = Log::default();
        let mut dom = VirtualDom::new_with_props(Cut, CutProps { log: log.clone() });
        Log::drive(&mut dom, || log.opened.borrow().len() >= 2).await;
        assert_eq!(log.items()[..1], [7]);
        assert!(log.states.borrow().contains(&Connection::Retrying));
    }
}
