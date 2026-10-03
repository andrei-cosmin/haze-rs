#![cfg(feature = "server")]

use std::future::{Ready, ready};

use dioxus::{
    fullstack::{
        FullstackContext,
        http::{Request, StatusCode, request::Parts},
    },
    server::axum::{
        Extension, Router,
        body::{Body, to_bytes},
        routing::get,
    },
};
use haze::Resources;
use tower::ServiceExt;

#[derive(Clone)]
struct Motto(&'static str);

struct Incoming;

impl Incoming {
    fn with(resources: Resources) -> Parts {
        let mut parts = Self::bare();
        parts.extensions.insert(resources);
        parts
    }

    fn bare() -> Parts {
        Request::new(()).into_parts().0
    }
}

struct Plain;

impl Plain {
    fn current() -> Ready<String> {
        ready(match Resources::current() {
            Ok(_) => String::from("found"),
            Err(error) => error.message.unwrap_or_default(),
        })
    }
}

#[tokio::test]
async fn the_current_registry_is_read_inside_a_request() {
    let mut resources = Resources::new();
    resources.insert(Motto("onward"));
    let motto = FullstackContext::new(Incoming::with(resources))
        .scope(async { Resources::current().unwrap().try_get::<Motto>().unwrap() })
        .await;
    assert_eq!(motto.0, "onward");
}

#[tokio::test]
async fn outside_a_request_it_is_a_500_that_says_so() {
    let error = Resources::current().unwrap_err();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        error.message.as_deref(),
        Some(
            "haze Resources were requested outside a server function or server render: during startup, in a task spawned from one, or in a plain axum handler or middleware, which takes Res<T> or Extension<Resources>"
        )
    );
}

#[tokio::test]
async fn a_plain_axum_handler_gets_the_same_500_while_handling_a_request() {
    let mut resources = Resources::new();
    resources.insert(Motto("onward"));
    let router = Router::new()
        .route("/current", get(Plain::current))
        .layer(Extension(resources));
    let response = router
        .oneshot(Request::get("/current").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        body,
        "haze Resources were requested outside a server function or server render: during startup, in a task spawned from one, or in a plain axum handler or middleware, which takes Res<T> or Extension<Resources>"
    );
}

#[tokio::test]
async fn a_request_without_a_registry_names_the_fix() {
    let error = FullstackContext::new(Incoming::bare())
        .scope(async { Resources::current().unwrap_err() })
        .await;
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        error.message.as_deref(),
        Some(
            "no haze Resources are attached to this router; start it with haze::serve or layer Extension(resources)"
        )
    );
}
