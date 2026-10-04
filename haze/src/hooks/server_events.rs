//! `use_server_events`: receives server-sent events and reconnects.

use std::{cell::Cell, rc::Rc};

use dioxus::{core::Task, prelude::*};
use dioxus_fullstack::{ServerEvents, ServerFnError, serde::de::DeserializeOwned};

use crate::hooks::{backoff::Backoff, connection::Connection, reconnect::Reconnect, timer::Idle};

/// Receives every event from a server-sent events server function, reconnecting
/// when the stream ends or fails.
///
/// `open` calls a server function that returns
/// [`ServerEvents<T>`](dioxus_fullstack::ServerEvents); `on_event` runs for each
/// event received; an event that fails to decode is skipped without dropping
/// the connection. Reconnects with the same backoff as
/// [`use_streaming`](crate::use_streaming).
/// Returns the current [`Connection`] state.
/// [`use_server_events_with`] adds an idle timeout and jitter.
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
    open: impl FnMut() -> Fut + 'static,
    on_event: impl FnMut(T) + 'static,
) -> ReadSignal<Connection>
where
    T: DeserializeOwned + 'static,
    E: 'static,
    Fut: Future<Output = Result<ServerEvents<T>, E>> + 'static,
{
    use_server_events_with(open, on_event, Reconnect::default())
}

/// [`use_server_events`] with [`Reconnect`] options: an idle timeout that drops
/// and reopens a stream that delivered no event for that long, and jitter on
/// the wait before each attempt. An event that fails to decode counts as
/// delivered.
///
/// `options` is read on the first render; later values are ignored. With
/// `Reconnect::default()` this is exactly [`use_server_events`].
///
/// # Examples
///
/// ```rust,ignore
/// let options = Reconnect { idle: Some(Duration::from_secs(45)), jitter: true };
/// let status = haze::use_server_events_with(notifications, move |toast| toasts.push(toast), options);
/// ```
#[track_caller]
pub fn use_server_events_with<T, E, Fut>(
    mut open: impl FnMut() -> Fut + 'static,
    mut on_event: impl FnMut(T) + 'static,
    options: Reconnect,
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
    let options = use_hook(|| options);
    use_effect(move || {
        let mut first = Some(open.call(()));
        if let Some(previous) = task.take() {
            previous.cancel();
        }
        task.set(Some(spawn(async move {
            let mut backoff = Backoff::new(options.jitter);
            loop {
                status.set(Connection::Connecting);
                let opening = first.take().unwrap_or_else(|| open.call(()));
                if let Ok(mut events) = opening.await {
                    status.set(Connection::Open);
                    let mut idle = Idle::new(options.idle);
                    loop {
                        match idle.within(events.recv()).await {
                            Some(Some(Ok(event))) => {
                                backoff.reset();
                                on_event.call(event);
                                idle.touch();
                            }
                            Some(Some(Err(ServerFnError::Serialization(_)))) => idle.touch(),
                            Some(Some(Err(_)) | None) | None => break,
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
    use crate::hooks::{test_log::Log, test_server::QuietServer};

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
        let opened = log.opened;
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

    #[component]
    fn JitteredAnnouncements(log: Log) -> Element {
        let opened = log.opened;
        use_server_events_with(
            move || {
                opened.borrow_mut().push(Instant::now());
                async { Ok::<_, Infallible>(ServerEvents::<u32>::new(|_| async {})) }
            },
            |_| {},
            Reconnect {
                idle: None,
                jitter: true,
            },
        );
        rsx! {}
    }

    #[component]
    fn QuietAnnouncements(log: Log, server: QuietServer, options: Reconnect) -> Element {
        let opened = log.opened.clone();
        let items = log.items.clone();
        let status = use_server_events_with(
            move || {
                opened.borrow_mut().push(Instant::now());
                server.open_events()
            },
            move |item| items.borrow_mut().push(item),
            options,
        );
        log.record(status());
        rsx! {}
    }

    #[component]
    fn SteadyAnnouncements(log: Log, server: QuietServer) -> Element {
        let opened = log.opened;
        let items = log.items;
        use_server_events_with(
            move || {
                opened.borrow_mut().push(Instant::now());
                server.open_steady_events()
            },
            move |item| items.borrow_mut().push(item),
            Reconnect {
                idle: Some(QuietServer::STEADY_GAP * 3 / 2),
                jitter: false,
            },
        );
        rsx! {}
    }

    #[tokio::test]
    async fn events_within_the_idle_timeout_keep_the_stream_open_past_one_that_fails_to_decode() {
        let log = Log::default();
        let props = SteadyAnnouncementsProps {
            log: log.clone(),
            server: QuietServer::start().await,
        };
        let mut dom = VirtualDom::new_with_props(SteadyAnnouncements, props);
        Log::drive(&mut dom, || log.items().len() >= 3).await;
        assert_eq!(*log.items(), [1, 2, 3]);
        assert_eq!(log.opened.borrow().len(), 1);
    }

    #[tokio::test]
    async fn a_quiet_event_stream_is_dropped_after_the_idle_timeout_and_reopened() {
        let log = Log::default();
        let props = QuietAnnouncementsProps {
            log: log.clone(),
            server: QuietServer::start().await,
            options: Reconnect {
                idle: Some(Duration::from_millis(300)),
                jitter: false,
            },
        };
        let mut dom = VirtualDom::new_with_props(QuietAnnouncements, props);
        Log::drive(&mut dom, || log.opened.borrow().len() >= 2).await;
        let gap = log.spacing()[0];
        assert!(gap >= Duration::from_millis(1300) && gap < Duration::from_millis(2500));
        assert_eq!(log.items()[..1], [1]);
        assert!(log.states.borrow().contains(&Connection::Retrying));
    }

    #[tokio::test]
    async fn without_an_idle_timeout_a_quiet_event_stream_stays_open() {
        let log = Log::default();
        let props = QuietAnnouncementsProps {
            log: log.clone(),
            server: QuietServer::start().await,
            options: Reconnect::default(),
        };
        let mut dom = VirtualDom::new_with_props(QuietAnnouncements, props);
        let deadline = Instant::now() + Duration::from_millis(1500);
        Log::drive(&mut dom, || Instant::now() >= deadline).await;
        assert_eq!(log.opened.borrow().len(), 1);
        assert_eq!(*log.items(), [1]);
        assert_eq!(*log.states.borrow().last().unwrap(), Connection::Open);
    }

    #[tokio::test(start_paused = true)]
    async fn with_jitter_two_event_clients_wait_differently_within_half_and_all_of_each_wait() {
        let first = Log::default();
        let second = Log::default();
        for log in [&first, &second] {
            let props = JitteredAnnouncementsProps { log: log.clone() };
            let mut dom = VirtualDom::new_with_props(JitteredAnnouncements, props);
            Log::drive(&mut dom, || log.opened.borrow().len() >= 4).await;
        }
        assert!(Log::within_jitter(&first.spacing(), &[1, 2, 4]));
        assert!(Log::within_jitter(&second.spacing(), &[1, 2, 4]));
        assert_ne!(first.spacing()[..3], second.spacing()[..3]);
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
