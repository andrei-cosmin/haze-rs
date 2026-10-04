//! Small helpers for [Dioxus] apps.
//!
//! haze builds typed resources once, injects them by type, and keeps live
//! streams from the server connected. It wraps Dioxus rather than replacing it:
//! routing, server functions, SSR, assets and hot reload stay exactly as Dioxus
//! documents them.
//!
//! # Three ways to run
//!
//! One set of resources and server functions serves three builds, chosen by
//! the application's own features:
//!
//! - Fullstack, with `server` on the server and `web` in the browser:
//!   [`serve`](fn@serve) runs startup once and attaches the registry to the
//!   application's Dioxus router, in any mode Dioxus supports. Server functions
//!   take a resource or a pack as `Res<T>`, an axum extractor that reads that
//!   registry, and [`Resources::current`] reads it inside a server function
//!   or server render. [`client_rendered`] is a ready-made router for pages
//!   drawn in the browser instead of on the server.
//! - Standalone, with `standalone`, in one process without a server:
//!   [`launch`](fn@launch) runs startup once, makes the registry the process
//!   default and launches the Dioxus app
//!   on desktop, mobile, native or web. Every [`#[server]`](macro@server)
//!   function then runs in process and reads its handles from that default,
//!   [`Resources::get_default`]. [`install`](fn@install) does the same
//!   without launching Dioxus.
//! - Core, with no feature: the registry and everything that fills it are
//!   plain Rust on any target, and [`Resources::start`] returns a started
//!   registry, for tests, tools or a custom launcher.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use dioxus::prelude::*;
//! #[cfg(feature = "server")]
//! use {
//!     haze::Res,
//!     std::sync::{Arc, atomic::{AtomicU64, Ordering}},
//! };
//!
//! #[cfg(feature = "server")]
//! #[derive(Clone, Default)]
//! struct Clicks(Arc<AtomicU64>);
//!
//! #[cfg(feature = "server")]
//! impl Clicks {
//!     fn bump(&self) -> u64 {
//!         self.0.fetch_add(1, Ordering::Relaxed) + 1
//!     }
//! }
//!
//! fn main() {
//!     #[cfg(not(feature = "server"))]
//!     dioxus::launch(App);
//!
//!     #[cfg(feature = "server")]
//!     haze::serve(
//!         async |resources| {
//!             resources.insert(Clicks::default());
//!             Ok(())
//!         },
//!         || dioxus::server::router(App),
//!     );
//! }
//!
//! #[server(clicks: Res<Clicks>)]
//! async fn bump() -> Result<u64> {
//!     Ok(clicks.bump())
//! }
//!
//! #[component]
//! fn App() -> Element {
//!     rsx! { button { onclick: move |_| async move { _ = bump().await; }, "+1" } }
//! }
//! ```
//!
//! # Standalone quick start
//!
//! The same counter as a desktop app without a server. The application
//! declares a `standalone` feature that enables `haze/standalone`, on by
//! default here, and picks the renderer on its own `dioxus` dependency:
//!
//! ```toml
//! [dependencies]
//! dioxus = { version = "0.7.10", features = ["desktop"] }
//! haze = "0.8"
//!
//! [features]
//! default = ["standalone"]
//! standalone = ["haze/standalone"]
//! ```
//!
//! ```rust,ignore
//! use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
//!
//! use dioxus::prelude::*;
//!
//! #[derive(Clone, Default)]
//! struct Clicks(Arc<AtomicU64>);
//!
//! impl Clicks {
//!     fn bump(&self) -> u64 {
//!         self.0.fetch_add(1, Ordering::Relaxed) + 1
//!     }
//! }
//!
//! fn main() {
//!     haze::launch(
//!         async |resources| {
//!             resources.insert(Clicks::default());
//!             Ok(())
//!         },
//!         LaunchBuilder::new(),
//!         App,
//!     );
//! }
//!
//! #[haze::server(clicks: Clicks)]
//! async fn bump() -> Result<u64> {
//!     Ok(clicks.bump())
//! }
//!
//! #[component]
//! fn App() -> Element {
//!     rsx! { button { onclick: move |_| async move { _ = bump().await; }, "+1" } }
//! }
//! ```
//!
//! For the browser, enable `web` instead of `desktop`, and keep `dioxus`'s
//! `fullstack` feature off: it turns on hydration, which reads a page
//! rendered by a server.
//!
//! # Resources
//!
//! The registry and everything that fills it are plain Rust, need no feature
//! and build for `wasm32` too:
//!
//! - [`Resources`] holds one value per type, inserted by hand or by startup,
//!   and lends each one out by reference.
//! - [`#[resource]`](macro@resource) turns a function into a resource: its
//!   `T` and `Option<T>` parameters are fetched from the registry and what it
//!   returns is inserted. [`Resources::provide`] runs them in dependency
//!   order.
//! - [`Later<T>`](struct@Later) is a resource-function parameter or pack field
//!   that is not waited for and is filled by the end of startup
//!   ([`Resources::finish`]), so two resources can need each other.
//! - [`#[register]`](macro@register) marks an `impl Trait for Type` so startup
//!   obtains every implementation of a trait into a [`Seq`], in `order`, after
//!   the resource functions, building `Build` types and cloning inserted values
//!   and packs ([`Resources::collect`] does it by hand).
//! - [`#[collect]`](macro@collect) declares such a trait, so its [`Seq`] exists,
//!   empty, when no implementation is linked, and `#[collect(N)]` makes
//!   startup fail unless exactly `N` implementations are registered.
//! - [`#[derive(Pack)]`](derive@Pack) implements [`Pack`](trait@Pack) for a
//!   struct of several resources, built once at startup and inserted, so shared
//!   logic can live in its methods.
//! - [`Resources::start`] runs all of startup: a setup closure, the resource
//!   functions, the registered traits and packs, then fills every `Later`.
//! - [`Res<T>`](struct@Res) wraps one resource handed to a server function.
//!
//! # Server functions
//!
//! [`#[server]`](macro@server) and the route attributes
//! [`#[get]`](macro@get), [`#[post]`](macro@post), [`#[put]`](macro@put),
//! [`#[delete]`](macro@delete) and [`#[patch]`](macro@patch) take Dioxus's
//! arguments followed by handles, `name: Type`, that are resources. One function
//! runs both ways:
//!
//! - In a fullstack build, it is Dioxus's server function, and its handles are
//!   extracted as `Res<T>` from the registry that [`serve`](fn@serve) attaches.
//! - With the application's `standalone` feature, it is an ordinary async
//!   function, and its handles come from the registry that
//!   [`launch`](fn@launch) installs.
//!
//! `standalone` takes precedence: a build with both `server` and `standalone`
//! compiles the in-process copy, so Dioxus registers none of these functions
//! and the server answers their routes with `404`. Never enable both in a
//! running build, which `--all-features` or a workspace's feature unification
//! can do.
//!
//! ```rust,ignore
//! #[haze::server(clicks: Clicks)]
//! async fn bump() -> Result<u64, ServerFnError> {
//!     Ok(clicks.bump())
//! }
//! ```
//!
//! # Client
//!
//! One hook per Dioxus push type, each reconnecting with backoff when the
//! connection drops:
//!
//! | Server returns | Hook |
//! |---|---|
//! | [`Streaming<T>`](dioxus_fullstack::Streaming) | [`use_streaming`] |
//! | [`ServerEvents<T>`](dioxus_fullstack::ServerEvents) | [`use_server_events`] |
//! | [`Websocket<In, Out>`](dioxus_fullstack::Websocket) | [`use_websocket`] |
//!
//! # Feature flags
//!
//! No features are enabled by default. Without any, haze depends on `dioxus`
//! (hooks and signals only), `dioxus-fullstack` and, on native targets,
//! tokio's timers. [`Resources`], [`Seq`], [`Pack`](trait@Pack),
//! [`Build`], [`Later`](struct@Later), [`Res`](struct@Res),
//! [`#[resource]`](macro@resource), [`#[register]`](macro@register),
//! [`#[collect]`](macro@collect), [`#[derive(Pack)]`](derive@Pack) and the
//! server function attributes are
//! always available, as are the client hooks.
//!
//! - `server`: [`serve`](fn@serve), [`client_rendered`],
//!   [`Resources::current`], and [`Res`](struct@Res) as an axum extractor.
//!   Enable it from the application's own `server` feature.
//! - `standalone`: [`launch`](fn@launch) and, on native targets,
//!   [`install`](fn@install), built on Dioxus's `LaunchBuilder`, the tokio
//!   runtime the caller entered or one haze builds, and `wasm-bindgen-futures`
//!   on `wasm32`, for an application that runs without a server and reads its
//!   resources from the process default. Enable it from the application's own
//!   `standalone` feature, which also selects the in-process copy of every
//!   server function attribute.
//!
//! haze depends on `dioxus-fullstack` directly and never turns on `dioxus`'s
//! `fullstack` feature, so a client-rendered app keeps hydration off (see
//! [`client_rendered`]).
//!
//! # Feature crates
//!
//! A feature crate exports its [`#[resource]`](macro@resource) functions,
//! [`#[register]`](macro@register) implementations,
//! [`#[collect]`](macro@collect) traits,
//! [`#[derive(Pack)]`](derive@Pack) structs and server functions. Startup
//! discovers them in every crate linked into the binary that runs it. Rust
//! links a crate only when the binary names it, so a crate that only provides
//! registrations must be named in `main.rs`:
//!
//! ```ignore
//! use that_crate as _;
//! ```
//!
//! Enabling `#![warn(unused_crate_dependencies)]` lists every dependency that
//! needs this line. A crate that uses the server function attributes follows
//! these rules:
//!
//! - haze is a non-optional dependency, because the attributes expand in every
//!   build. The crate's `server` feature enables `haze/server`.
//! - A crate that also runs standalone declares a `standalone` feature that
//!   enables `haze/standalone`. Once any crate in the build enables it, the
//!   attributes emit the in-process copy under that feature in every crate,
//!   so each of them needs the feature or the build fails with an
//!   `unexpected_cfgs` error. A fullstack-only crate declares nothing.
//! - `server` and `standalone` are never enabled in the same build.
//!   `standalone` takes precedence, and the server then answers those routes
//!   with `404`.
//!
//! ```toml
//! [dependencies]
//! dioxus-fullstack = { version = "0.7.10", default-features = false }
//! dioxus-server = { version = "0.7.10", optional = true }
//! haze = "0.8"
//!
//! [features]
//! server = ["dioxus-fullstack/server", "dep:dioxus-server", "haze/server"]
//! standalone = ["haze/standalone"]
//! ```
//!
//! [Dioxus]: https://dioxuslabs.com

#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(
    not(all(feature = "server", feature = "standalone")),
    allow(rustdoc::broken_intra_doc_links)
)]
#![warn(missing_docs)]

pub mod hooks;
pub mod inject;
#[cfg(feature = "standalone")]
#[cfg_attr(docsrs, doc(cfg(feature = "standalone")))]
pub mod launch;
pub mod resources;
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod serve;

pub use haze_macros::{Pack, collect, delete, get, patch, post, put, register, resource, server};
pub use hooks::{
    Connection, Reconnect, use_server_events, use_server_events_with, use_streaming,
    use_streaming_with, use_websocket, use_websocket_with,
};
pub use inject::{Build, Later, Pack, Res};
#[cfg(all(feature = "standalone", not(target_family = "wasm")))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(feature = "standalone", not(target_family = "wasm"))))
)]
pub use launch::install;
#[cfg(feature = "standalone")]
#[cfg_attr(docsrs, doc(cfg(feature = "standalone")))]
pub use launch::launch;
pub use resources::{Resources, Seq};
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub use serve::{client_rendered, serve};

/// Not public API; referenced by the code `haze-macros` generates.
#[doc(hidden)]
pub mod __private {
    pub use anyhow;
    pub use dioxus_fullstack::ServerFnError;
    pub use inventory;

    pub use crate::inject::{
        cardinality::Cardinality,
        installer::Installer,
        need::Need,
        obtain::{Built, Inserted, Obtain},
        provider::Provider,
        registration::Registration,
    };

    /// The error tags `#[haze::resource]` glob-imports, the way `anyhow!`
    /// glob-imports its `__private::kind`.
    #[doc(hidden)]
    pub mod error_kind {
        pub use crate::inject::error_kind::{CapturedKind, PlainKind};
    }
}
