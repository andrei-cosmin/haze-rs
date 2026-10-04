//! A local server for the hook tests: its quiet event stream and socket each
//! deliver one item and then go quiet, and its steady ones deliver an item,
//! one that fails to decode and two more, a gap apart.

use std::{future::pending, net::SocketAddr, time::Duration};

use dioxus_fullstack::{
    ClientRequest, FromResponse, HeaderMap, IntoRequest, Message, Method, ServerEvents,
    ServerFnError, TypedWebsocket, WebSocketOptions, Websocket,
    axum::{
        BoxError, Router,
        response::sse::{Event, Sse},
        routing::get,
    },
    http::Extensions,
};
use futures_util::{StreamExt, stream};
use tokio::net::TcpListener;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct QuietServer {
    address: SocketAddr,
}

impl QuietServer {
    pub(crate) const STEADY_GAP: Duration = Duration::from_millis(400);
    const STEADY_ITEMS: [&str; 4] = ["1", "oops", "2", "3"];

    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .route("/events", get(|| async { Self::events() }))
            .route("/steady-events", get(|| async { Self::steady_events() }))
            .route(
                "/socket",
                get(|options: WebSocketOptions| async { Self::socket(options) }),
            )
            .route(
                "/steady-socket",
                get(|options: WebSocketOptions| async { Self::steady_socket(options) }),
            );
        tokio::spawn(async move { dioxus_fullstack::axum::serve(listener, router).await });
        Self { address }
    }

    fn events() -> ServerEvents<u32> {
        ServerEvents::new(|mut sender| async move {
            sender.send(1).await.ok();
            pending::<()>().await;
        })
    }

    fn steady_events() -> ServerEvents<u32> {
        let events = stream::iter(Self::STEADY_ITEMS).then(|item| async move {
            tokio::time::sleep(Self::STEADY_GAP).await;
            Ok::<_, BoxError>(Event::default().data(item))
        });
        ServerEvents::from_sse(Sse::new(events.chain(stream::pending()).boxed()))
    }

    fn socket(options: WebSocketOptions) -> Websocket<String, u32> {
        options.on_upgrade(|mut socket: TypedWebsocket<String, u32>| async move {
            socket.send(1).await.ok();
            pending::<()>().await;
        })
    }

    fn steady_socket(options: WebSocketOptions) -> Websocket<String, u32> {
        options.on_upgrade(|mut socket: TypedWebsocket<String, u32>| async move {
            for item in Self::STEADY_ITEMS {
                tokio::time::sleep(Self::STEADY_GAP).await;
                socket.send_raw(Message::Text(item.into())).await.ok();
            }
            pending::<()>().await;
        })
    }

    fn request(self, path: &str) -> ClientRequest {
        ClientRequest {
            url: format!("http://{}{path}", self.address).parse().unwrap(),
            headers: HeaderMap::new(),
            method: Method::GET,
            extensions: Extensions::new(),
        }
    }

    pub(crate) async fn open_events(self) -> Result<ServerEvents<u32>, ServerFnError> {
        self.events_at("/events").await
    }

    pub(crate) async fn open_steady_events(self) -> Result<ServerEvents<u32>, ServerFnError> {
        self.events_at("/steady-events").await
    }

    pub(crate) async fn open_socket(self) -> Result<Websocket<String, u32>, ServerFnError> {
        self.socket_at("/socket").await
    }

    pub(crate) async fn open_steady_socket(self) -> Result<Websocket<String, u32>, ServerFnError> {
        self.socket_at("/steady-socket").await
    }

    async fn events_at(self, path: &str) -> Result<ServerEvents<u32>, ServerFnError> {
        let response = self
            .request(path)
            .send_empty_body()
            .await
            .map_err(ServerFnError::from)?;
        ServerEvents::from_response(response).await
    }

    async fn socket_at(self, path: &str) -> Result<Websocket<String, u32>, ServerFnError> {
        let upgrading = WebSocketOptions::new()
            .into_request(self.request(path))
            .await
            .map_err(ServerFnError::from)?;
        Websocket::from_response(upgrading).await
    }
}
