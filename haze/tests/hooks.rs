#![cfg(feature = "hooks")]

use std::{cell::RefCell, convert::Infallible, rc::Rc, time::Duration};

#[cfg(feature = "server")]
use dioxus::fullstack::ServerEvents;
use dioxus::{
    core::NoOpMutations,
    fullstack::{
        Encoding, JsonEncoding, Streaming,
        body::Body,
        encode_stream_frame,
        extract::{FromRequest, Request},
        http::header::CONTENT_TYPE,
    },
    prelude::*,
};
use futures_util::{Stream, StreamExt, stream};
use haze::Connection;
use tokio::time::Instant;

#[derive(Clone, Default)]
struct Log {
    items: Rc<RefCell<Vec<u32>>>,
    states: Rc<RefCell<Vec<Connection>>>,
    opened: Rc<RefCell<Vec<Instant>>>,
    handles: Rc<RefCell<Vec<ReadSignal<Connection>>>>,
}

impl PartialEq for Log {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }
}

impl Log {
    fn items(&self) -> Vec<u32> {
        self.items.borrow().clone()
    }

    fn gaps(&self) -> Vec<u64> {
        let opened = self.opened.borrow();
        let mut gaps = Vec::new();
        for pair in opened.windows(2) {
            gaps.push((pair[1] - pair[0]).as_secs());
        }
        gaps
    }

    fn record(&self, status: Connection) {
        let mut seen = self.states.borrow_mut();
        if seen.last() != Some(&status) {
            seen.push(status);
        }
    }

    fn watch(&self, status: ReadSignal<Connection>) {
        self.handles.borrow_mut().push(status);
        self.record(status());
    }

    fn same_handle_every_render(&self) -> bool {
        let handles = self.handles.borrow();
        handles.len() >= 3 && handles.iter().all(|handle| Some(handle) == handles.first())
    }

    fn one_now_two_later() -> impl Stream<Item = u32> {
        let two = stream::once(async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            2
        });
        stream::iter([1]).chain(two).chain(stream::pending())
    }

    async fn drive(dom: &mut VirtualDom, until: impl Fn() -> bool) {
        dom.rebuild_in_place();
        for _ in 0..400 {
            if until() {
                return;
            }
            tokio::select! {
                () = dom.wait_for_work() => {}
                () = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            dom.render_immediate(&mut NoOpMutations);
        }
        panic!("the hook never reached the expected state");
    }
}

#[component]
fn Counter(log: Log) -> Element {
    let opened = log.opened.clone();
    let items = log.items.clone();
    let status = haze::use_streaming(
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
    let status = haze::use_streaming(
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
    let status = haze::use_streaming(
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
    let status = haze::use_streaming(
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
    haze::use_streaming(
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
    haze::use_streaming(
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
    haze::use_streaming(
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
    let status = haze::use_streaming(
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
    haze::use_streaming(
        || async { Ok::<_, Infallible>(Streaming::<u32, ()>::new(Log::one_now_two_later())) },
        move |item| {
            items.borrow_mut().push(item);
            hidden.set(true);
        },
    );
    rsx! {}
}

#[cfg(feature = "server")]
#[component]
fn Announcements(log: Log) -> Element {
    let opened = log.opened.clone();
    let status = haze::use_server_events(
        move || {
            opened.borrow_mut().push(Instant::now());
            async { Ok::<_, Infallible>(ServerEvents::<u32>::new(|_| async {})) }
        },
        |_| {},
    );
    log.watch(status);
    rsx! {}
}

#[cfg(feature = "server")]
#[component]
fn HiddenAnnouncements(log: Log) -> Element {
    let mut hidden = use_signal(|| false);
    if hidden() {
        return rsx! {};
    }
    let opened = log.opened.clone();
    haze::use_server_events(
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

#[cfg(feature = "server")]
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

#[cfg(feature = "server")]
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
