#![cfg(all(feature = "server", feature = "standalone"))]
#![allow(clippy::unused_async)]

use dioxus::{
    fullstack::{
        ServerFnError,
        http::{Request, StatusCode, header::CONTENT_TYPE},
    },
    prelude::{dioxus_server, server},
    server::{
        DioxusRouterExt, FullstackState,
        axum::{
            Extension, Router,
            body::{Body, to_bytes},
        },
    },
};
use haze::Resources;
use tower::ServiceExt;

#[derive(Clone)]
struct Season(&'static str);

#[server(endpoint = "plain")]
async fn plain() -> Result<String, ServerFnError> {
    Ok(String::from("dioxus"))
}

#[haze::server(endpoint = "season", season: Season)]
async fn season() -> Result<String, ServerFnError> {
    Ok(season.0.to_owned())
}

struct Server;

impl Server {
    async fn router() -> Router {
        let resources = Resources::start(async |resources| {
            resources.insert(Season("spring"));
            Ok(())
        })
        .await
        .unwrap();
        Router::new()
            .register_server_functions()
            .with_state(FullstackState::headless())
            .layer(Extension(resources))
    }

    async fn post(path: &str) -> (StatusCode, String) {
        let request = Request::post(path)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let response = Self::router().await.oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }
}

#[tokio::test]
async fn dioxus_still_serves_its_own_server_functions() {
    assert_eq!(
        Server::post("/api/plain").await,
        (StatusCode::OK, String::from("\"dioxus\""))
    );
}

#[tokio::test]
async fn dioxus_serves_no_route_for_a_haze_server_function() {
    assert_eq!(
        Server::post("/api/season").await,
        (StatusCode::NOT_FOUND, String::new())
    );
}

#[tokio::test]
async fn a_haze_server_function_is_the_in_process_copy() {
    assert_eq!(
        season().await.unwrap_err(),
        ServerFnError::ServerError {
            message: String::from(
                "season was called, but no haze Resources are installed; call Resources::install_default, which haze::launch does, before calling it"
            ),
            code: 500,
            details: None,
        }
    );
}
