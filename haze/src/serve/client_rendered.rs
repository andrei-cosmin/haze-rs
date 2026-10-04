//! `client_rendered`: a Dioxus router that draws pages in the browser.

use std::{future::ready, path::PathBuf};

use dioxus::server::{
    DioxusRouterExt, FullstackState,
    axum::{
        Router,
        http::header::CACHE_CONTROL,
        response::{Html, IntoResponse},
        routing::get,
    },
};

/// A Dioxus router that serves client-rendered pages: the app's server
/// functions, its static assets, and the generated `index.html` for every other
/// path, so the browser draws the UI instead of the server.
///
/// Built only from Dioxus's public router pieces. Use it as the router for
/// [`serve`](fn@crate::serve) when server-side rendering is not wanted. The page
/// is sent with `Cache-Control: no-cache`, so browsers pick up a new build.
///
/// The page and the assets come from one public folder, found the way Dioxus
/// finds it: `DIOXUS_PUBLIC_PATH`, or the `public` folder next to the server
/// executable, which is where `dx` puts it.
/// A `base_path` from `Dioxus.toml` is honored the way [`dioxus::server::router`]
/// honors it: everything is served under `/{base_path}/`.
///
/// # Setup
///
/// - The web client must not enable `dioxus`'s `fullstack` feature. It switches
///   on `dioxus-web`'s hydration, which expects a server-rendered page and fails
///   on this one.
/// - The client depends on `dioxus` with `web`, on the `dioxus-fullstack` crate
///   for `#[server]`, and enables haze's `web` feature.
/// - The server build enables `dioxus/server`, `dioxus-fullstack/server` and
///   haze's `server` feature.
/// - On the server, `#[server]` expands to paths through `dioxus_server`, which
///   `use dioxus::prelude::*` brings into scope. A module without that import
///   adds `use dioxus::prelude::dioxus_server;`.
///
/// # Panics
///
/// Panics if the public folder cannot be identified or its generated
/// `index.html` cannot be read.
///
/// # Examples
///
/// ```rust,ignore
/// haze::serve(
///     async |resources| {
///         resources.insert(Clicks::default());
///         Ok(())
///     },
///     haze::client_rendered,
/// );
/// ```
pub fn client_rendered() -> Router {
    let public = ClientRendered::public_path()
        .expect("cannot identify the public folder; set DIOXUS_PUBLIC_PATH");
    let path = public.join("index.html");
    let page = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let base = dioxus::cli_config::base_path().unwrap_or_default();
    let routes = Router::<FullstackState>::new()
        .register_server_functions()
        .serve_static_assets();
    ClientRendered::router(page, &base, routes)
}

/// The pieces of [`client_rendered`].
///
/// `public_path` mirrors Dioxus's own lookup behind `serve_static_assets`, so
/// the page is read from the folder the assets are served from. `router` takes
/// the page, the base path and the routes, so the routing can be tested without
/// reading the environment or the public folder, the way Dioxus's own
/// `apply_base_path` takes the base path as an argument.
struct ClientRendered;

impl ClientRendered {
    /// Finds the public folder the way Dioxus does: `DIOXUS_PUBLIC_PATH`, or the
    /// `public` folder next to the server executable, where `dx` bundles the
    /// assets. `None` when the executable's path is unknown, in which case
    /// Dioxus serves no assets either.
    fn public_path() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("DIOXUS_PUBLIC_PATH") {
            return Some(PathBuf::from(path));
        }
        let executable = std::env::current_exe().ok()?;
        Some(executable.parent()?.join("public"))
    }

    /// Serves `routes`, and `page` for every other path, nested under `base`
    /// when it is not empty. [`client_rendered`] passes Dioxus's server function
    /// and static asset routes, which need the public folder to exist; the tests
    /// pass an empty router.
    fn router(page: String, base: &str, routes: Router<FullstackState>) -> Router {
        let index = get(move || ready(([(CACHE_CONTROL, "no-cache")], Html(page)).into_response()));
        let state = FullstackState::headless();
        let app = routes.fallback(index.clone()).with_state(state.clone());
        let base = base.trim_matches('/');
        if base.is_empty() {
            return app;
        }
        Router::new()
            .nest(&format!("/{base}/"), app)
            .route(&format!("/{base}"), index.with_state(state))
    }
}

#[cfg(test)]
mod tests {
    use dioxus::server::{
        FullstackState,
        axum::{
            Router,
            body::{Body, to_bytes},
            http::{Request, StatusCode, header::CACHE_CONTROL},
            routing::get,
        },
    };
    use tower::ServiceExt;

    use super::ClientRendered;

    struct Visit;

    impl Visit {
        async fn page(router: Router, path: &str) -> (StatusCode, Option<String>, String) {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();
            let response = router.oneshot(request).await.unwrap();
            let status = response.status();
            let cache = response
                .headers()
                .get(CACHE_CONTROL)
                .map(|value| value.to_str().unwrap().to_owned());
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (status, cache, String::from_utf8(body.to_vec()).unwrap())
        }
    }

    #[tokio::test]
    async fn every_path_gets_the_page_without_caching() {
        let router = ClientRendered::router(String::from("<main>app</main>"), "", Router::new());
        let (status, cache, body) = Visit::page(router, "/some/deep/page").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache.as_deref(), Some("no-cache"));
        assert_eq!(body, "<main>app</main>");
    }

    #[tokio::test]
    async fn a_base_path_serves_under_it_with_and_without_a_trailing_slash() {
        let router = ClientRendered::router(String::from("page"), "/relay/", Router::new());
        for path in ["/relay", "/relay/", "/relay/monitor"] {
            let (status, _, body) = Visit::page(router.clone(), path).await;
            assert_eq!((status, body.as_str()), (StatusCode::OK, "page"), "{path}");
        }
        let (status, _, _) = Visit::page(router, "/monitor").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_routes_are_served_before_the_page_with_and_without_a_base_path() {
        for (base, prefix) in [("", ""), ("/relay/", "/relay")] {
            let api = Router::<FullstackState>::new().route("/api/ping", get(|| async { "pong" }));
            let router = ClientRendered::router(String::from("page"), base, api);
            let pong = Visit::page(router.clone(), &format!("{prefix}/api/ping")).await;
            assert_eq!(pong, (StatusCode::OK, None, String::from("pong")), "{base}");
            let page = Visit::page(router, &format!("{prefix}/other")).await;
            assert_eq!(
                page,
                (
                    StatusCode::OK,
                    Some(String::from("no-cache")),
                    String::from("page")
                ),
                "{base}"
            );
        }
    }
}
