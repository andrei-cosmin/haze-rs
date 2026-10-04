#![cfg(all(feature = "server", not(feature = "standalone")))]

use dioxus::{
    fullstack::{
        ServerFnError,
        http::{Request, StatusCode, header::CONTENT_TYPE},
    },
    prelude::dioxus_server,
    server::{
        DioxusRouterExt, FullstackState,
        axum::{
            Extension, Router,
            body::{Body, to_bytes},
        },
    },
};
use haze::{Pack, Res, Resources};
use tower::ServiceExt;

#[derive(Clone)]
struct Season(&'static str);

#[derive(Clone)]
struct Absent;

#[derive(Clone, Pack)]
struct Board {
    season: Season,
}

#[haze::server(endpoint = "season", board: Board)]
async fn season() -> Result<String, ServerFnError> {
    Ok(board.season.0.to_owned())
}

#[haze::server(endpoint = "optional", board: Option<Board>, absent: Option<Absent>)]
async fn optional() -> Result<(bool, bool), ServerFnError> {
    Ok((board.is_some(), absent.is_some()))
}

#[haze::server(
    endpoint = "wrapped",
    board: Res<Board>,
    season: Option<Res<Season>>,
    absent: Option<Res<Absent>>
)]
async fn wrapped() -> Result<(String, bool, bool), ServerFnError> {
    let Res(board) = board;
    Ok((
        board.season.0.to_owned(),
        season.is_some(),
        absent.is_some(),
    ))
}

#[haze::server(endpoint = "missing", absent: Absent)]
async fn missing() -> Result<(), ServerFnError> {
    let Absent = absent;
    Ok(())
}

#[haze::get("/api/page?page", board: Board)]
async fn paged(page: u32) -> Result<String, ServerFnError> {
    Ok(format!("{} page {page}", board.season.0))
}

#[haze::post("/api/hit", board: Board)]
async fn hit(times: u64) -> Result<String, ServerFnError> {
    Ok(format!("{} hit {times}", board.season.0))
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

    async fn call(request: Request<Body>) -> (StatusCode, String) {
        let response = Self::router().await.oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    async fn post(path: &str, json: &'static str) -> (StatusCode, String) {
        Self::call(
            Request::post(path)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json))
                .unwrap(),
        )
        .await
    }
}

#[tokio::test]
async fn a_pack_handle_is_extracted_through_res_over_a_request() {
    assert_eq!(
        Server::post("/api/season", "{}").await,
        (StatusCode::OK, String::from("\"spring\""))
    );
}

#[tokio::test]
async fn an_optional_handle_is_none_only_when_its_type_is_missing() {
    assert_eq!(
        Server::post("/api/optional", "{}").await,
        (StatusCode::OK, String::from("[true,false]"))
    );
}

#[tokio::test]
async fn a_res_handle_reaches_dioxus_as_written() {
    assert_eq!(
        Server::post("/api/wrapped", "{}").await,
        (StatusCode::OK, String::from("[\"spring\",true,false]"))
    );
}

#[tokio::test]
async fn a_missing_handle_is_the_res_rejection() {
    assert_eq!(
        Server::post("/api/missing", "{}").await,
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            String::from("server_attribute::Absent was never inserted")
        )
    );
}

#[tokio::test]
async fn a_route_query_parameter_stays_a_plain_argument() {
    assert_eq!(
        Server::call(
            Request::get("/api/page?page=3")
                .body(Body::empty())
                .unwrap()
        )
        .await,
        (StatusCode::OK, String::from("\"spring page 3\""))
    );
}

#[tokio::test]
async fn a_post_route_takes_its_arguments_and_its_handle() {
    assert_eq!(
        Server::post("/api/hit", "{\"times\":2}").await,
        (StatusCode::OK, String::from("\"spring hit 2\""))
    );
}

#[tokio::test]
async fn a_direct_call_outside_a_request_is_the_res_rejection() {
    assert_eq!(
        season().await.unwrap_err(),
        ServerFnError::ServerError {
            message: String::from(
                "server_attribute::Board was requested, but no haze Resources are attached to this request; start the router with haze::serve or layer Extension(resources), and call the server function inside a request"
            ),
            code: 500,
            details: None,
        }
    );
}
