# haze

[![CI](https://github.com/andrei-cosmin/haze-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/andrei-cosmin/haze-rs/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Small helpers for [Dioxus](https://dioxuslabs.com) apps, fullstack or standalone: register resources once, inject them into server functions by type, run the same server functions in one process without a server, and keep live streams connected.

haze wraps Dioxus instead of replacing it. Routing, server functions, SSR, assets and hot reload stay exactly as Dioxus documents them.

## Install

```toml
[dependencies]
dioxus = { version = "0.7.10", features = ["fullstack"] }
haze = { git = "https://github.com/andrei-cosmin/haze-rs", tag = "v0.8.0" }

[features]
web = ["dioxus/web", "haze/web"]
server = ["dioxus/server", "haze/server"]
```

This is the server-rendered setup, where `dioxus`'s `fullstack` feature turns on hydration. For pages drawn in the browser with `haze::client_rendered`, leave `fullstack` off and use the `dioxus-fullstack` crate for `#[server]`, as its documentation describes. Add `anyhow` as well for hand-written `Build` impls, for calling `Pack::build` yourself and for resource functions that return `anyhow::Result`.

For an app that runs in one process without a server, declare a `standalone` feature that enables `haze/standalone`, and pick the renderer on `dioxus` itself:

```toml
[dependencies]
dioxus = { version = "0.7.10", features = ["desktop"] }
haze = { git = "https://github.com/andrei-cosmin/haze-rs", tag = "v0.8.0" }

[features]
default = ["standalone"]
standalone = ["haze/standalone"]
```

`main` then calls `haze::launch` instead of `haze::serve`, and server functions use `#[haze::server]`; the crate documentation's *Standalone quick start* has the whole counter. For the browser, enable `web` instead of `desktop`, and keep `dioxus`'s `fullstack` feature off: it turns on hydration, which reads a page rendered by a server.

## Quick start

```rust
use dioxus::prelude::*;
#[cfg(feature = "server")]
use {
    haze::Res,
    std::sync::{Arc, atomic::{AtomicU64, Ordering}},
};

#[cfg(feature = "server")]
#[derive(Clone, Default)]
struct Clicks(Arc<AtomicU64>);

#[cfg(feature = "server")]
impl Clicks {
    fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }
}

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    haze::serve(
        async |resources| {
            resources.insert(Clicks::default());
            Ok(())
        },
        || dioxus::server::router(App),
    );
}

#[server(clicks: Res<Clicks>)]
async fn bump() -> Result<u64> {
    Ok(clicks.bump())
}

#[component]
fn App() -> Element {
    rsx! { button { onclick: move |_| async move { _ = bump().await; }, "+1" } }
}
```

Run it with `dx serve --web --fullstack`.

## What's inside

**Core** (no feature)

Plain Rust on every target, `wasm32` included, with no Dioxus, axum or tokio dependency. Startup is `Resources::start`, which `haze::serve` and `haze::launch` run once.

| Item | Does |
|---|---|
| `Resources` | Holds one value per type, inserted by hand or by startup, and hands out clones. `Resources::start` runs startup on its own, for tests and tools. `Resources::install_default` makes a started registry the process default, which `Resources::get_default` returns anywhere in the process. |
| `#[haze::resource]` | Turns a function into a resource. Its parameters, `T`, `Option<T>` or `Later<T>`, are fetched from the registry and what it returns is inserted; startup runs every such function in dependency order after setup, and stops with a clear error for a missing type, a failing function, two functions for one type or a cycle of required parameters. |
| `Later<T>` | A resource-function parameter or pack field filled by the end of startup instead of waited for, so two resources can need each other. |
| `#[derive(Pack)]` | Builds a `Clone` struct of several resources once at startup and inserts it, so logic shared by server functions lives in its methods and they take it as a handle, `#[haze::server(scores: Scores)]`, or as `Res<Scores>` with Dioxus's `#[server(scores: Res<Scores>)]`. Fields are fetched by type; `Option<T>` is `None` when `T` is absent when the pack is built (for a `Seq` or another pack that depends on install order, so prefer `T` or `Later<T>`); `Later<T>` closes a cycle; `#[pack(func = call(..))]` computes a field once, after the other fields, seeing every non-func field and the func fields above it. Startup fails if the type of a `T` or `Later<T>` field was never inserted. |
| `#[haze::register]` + `Seq<T>` | Mark `impl Trait for Type` with `#[haze::register(order = 20)]`; startup obtains every implementation in order after the resource functions and inserts them as `Seq<dyn Trait>`, injected like any resource. Each implementation must implement `haze::Build`, or be a `Clone` value inserted (in setup or by a `#[haze::resource]` function) or a `#[derive(Pack)]` struct; a `Clone` type that is none of these fails startup with `X was never inserted`. A registered pack is cloned from the one startup built. |
| `#[haze::server]`, `#[haze::get]`, `#[haze::post]`, `#[haze::put]`, `#[haze::delete]`, `#[haze::patch]` | Take Dioxus's arguments for `#[server]` or the route attribute of the same name, followed by handles `name: Type` that are resources: `T`, `Option<T>`, `Res<T>` or `Option<Res<T>>`. Without the app's `standalone` feature the function is Dioxus's own server function, its handles extracted as `Res<T>` from the registry `haze::serve` attaches. With it, the function is an ordinary async function that reads its handles from the process default, with no HTTP at all. A missing `T` fails the call with a `500` naming `T`, and a standalone call to a function with handles before a registry is installed fails with a `500` naming the function. |

**Server** (`server` feature)

| Item | Does |
|---|---|
| `haze::serve` | Runs a one-time async setup that fills the resource registry, then attaches it to the Dioxus router you build: server-rendered, client-rendered, streaming or with your own routes. Server hot-patches reuse the registry instead of rerunning setup. |
| `Res<T>` | Gives a server function a clone of one resource, with the same rules as axum's `Extension<T>`. `Option<Res<T>>` receives `None` when it is missing from the attached registry. Inside the body, `Resources::current()?.try_get::<T>()` reads the same registry, the way Dioxus's `FullstackContext::extension` reads a request extension. `#[haze::server]` hands its handles to Dioxus this way. |
| `haze::client_rendered` | A ready-made Dioxus router for pages drawn in the browser instead of on the server: `haze::serve(setup, haze::client_rendered)`. The client must not enable `dioxus/fullstack`, which forces hydration. |

**Standalone** (`standalone` feature)

| Item | Does |
|---|---|
| `haze::launch` | Runs startup, makes the registry the process default and launches the Dioxus app on desktop, mobile, native or web, `haze::launch(setup, App)`, so every `#[haze::server]` function runs in process. A failing startup panics with the error and its causes. |
| `haze::install` | Native targets only: runs startup on a multi-thread tokio runtime that haze keeps for the rest of the process, makes the registry the process default and returns it, for launching Dioxus with a `LaunchBuilder` of your own or for a program without a user interface. Startup runs once per process: a second call returns the same registry without running setup again. |

**Client** (`hooks` feature, enabled by `web`, `server` and `standalone`; every platform: web, desktop, mobile, native)

| Server function returns | Hook | Adds |
|---|---|---|
| `Streaming<T>` | `haze::use_streaming` | receiving every item, reconnecting |
| `ServerEvents<T>` | `haze::use_server_events` | receiving every event, reconnecting |
| `Websocket<In, Out>` | `haze::use_websocket` | reconnecting, on top of Dioxus's `use_websocket` |

Reconnects wait 1 second, doubling up to 30 seconds, and reset once a connection delivers an item. A client crate that enables none of those features, such as a desktop UI crate, depends on haze with `features = ["hooks"]`.

Only `Streaming<T>` works in every build. `ServerEvents<T>` and `Websocket<In, Out>` are an SSE response and an upgraded connection, which only a server can produce, so in a `standalone` build push from the backend with `Streaming<T>` and `use_streaming`: the items are handed over in process, without encoding.

## Feature crates

haze has no plugin type. A feature crate exports its `#[haze::resource]` functions, `#[haze::register]` impls, `#[derive(Pack)]` structs and `#[server]` functions, and nothing else; the app's setup inserts only what `main` alone knows, such as paths from the command line. haze finds resource functions, registered implementations and packs the way Dioxus finds server functions: in every crate linked into the binary that runs startup, the server binary of a fullstack app or the app itself in a standalone build. Rust links a crate only if something in the binary names it, and naming a type the crate provides but does not define is not enough. A crate that nothing names, such as one that only registers an implementation of another crate's trait or only provides a type defined elsewhere, is silently absent: its functions never run, an `Option<T>` of its type receives `None`, and a required `T` of its type stops startup saying nothing provides it. Name such a crate in `main.rs` with `use that_crate as _;`, under `#[cfg(feature = "server")]` when it is a server-only dependency. rustc's `unused_crate_dependencies` lint is off by default; enabled with `#![warn(unused_crate_dependencies)]`, it lists every dependency `main.rs` does not name and suggests that line.

A feature crate that uses `#[haze::server]` or the route attributes follows four rules:

- The attributes expand in every build, the client's included, so haze is a non-optional dependency of the crate, and its `server` feature enables `haze/server`.
- Each such crate declares a `standalone` feature that enables `haze/standalone`, the way Dioxus asks for a `server` feature. Without it, the build fails at every attribute with an `unexpected_cfgs` error whose note shows the line to add, the way Dioxus's own attribute reports a missing `server` feature. `standalone` takes precedence: a build with both `server` and `standalone` compiles the in-process copy, so Dioxus registers none of these functions and the server answers their routes with `404`. Never enable both in a build you run, which `--all-features` or a workspace's feature unification can do.
- haze with no features exports no client hooks: a crate that calls `use_server_events`, `use_streaming` or `use_websocket` without enabling `web` or `server` depends on haze with `features = ["hooks"]`.
- A `#[derive(Pack)]` struct is not an axum extractor on its own: take it as a handle of `#[haze::server(scores: Scores)]`, or as `Res<Scores>` with Dioxus's `#[server(scores: Res<Scores>)]`.

A feature crate's manifest then reads:

```toml
[dependencies]
dioxus-fullstack = { version = "0.7.10", default-features = false }
dioxus-server = { version = "0.7.10", optional = true }
haze = { git = "https://github.com/andrei-cosmin/haze-rs", tag = "v0.8.0" }

[features]
server = ["dioxus-fullstack/server", "dep:dioxus-server", "haze/server"]
web = ["dioxus-fullstack/web"]
standalone = ["haze/standalone"]
```

## Examples

In [`haze-examples/examples`](haze-examples/examples), each run from the `haze-examples` folder:

| Example | Shows | Run |
|---|---|---|
| `counter` | `Res<T>`, and `#[derive(Pack)]` taken as `Res<Pack>` | `dx serve --example counter --web --fullstack` |
| `streaming` | `use_streaming` | `dx serve --example streaming --web --fullstack` |
| `server_events` | `use_server_events` | `dx serve --example server_events --web --fullstack` |
| `websocket` | `use_websocket` | `dx serve --example websocket --web --fullstack` |
| `register` | `#[haze::register]` and `Seq<T>` | `dx serve --example register --web --fullstack` |
| `resource` | `#[haze::resource]` with `T`, `Option<T>` and `Later<T>` | `dx serve --example resource --web --fullstack` |
| `standalone` | `haze::launch` and `#[haze::server]` in one desktop process, without a server | `cargo run --example standalone --features standalone,dioxus/desktop` |

## Development

CI runs on every push to `main`, every pull request and before every release:

| Job | Checks |
|---|---|
| checks | `cargo fmt`, `cargo clippy` with warnings denied: the workspace with all features, `haze` with none, `haze`'s server side without `dioxus/fullstack`, and `haze` with its tests under each of `hooks`, `web`, `server` and `standalone` |
| test | tests on Linux, macOS and Windows: the workspace with no and with all features, `haze`'s own with no features as `cargo test -p haze --no-default-features`, since the examples turn `hooks` on across the workspace, and `haze`'s tests under each of `hooks`, `web`, `server` and `standalone`; on macOS the `standalone` example builds with Dioxus's desktop renderer |
| wasm | the library builds for `wasm32-unknown-unknown` with no features, with `web` and with `standalone`, the examples' web clients check, and the `standalone` example checks with `standalone` and Dioxus's web renderer, without hydration |
| docs | rustdoc with warnings denied, with all and with no features, and doc tests |

`haze`'s tests of `#[haze::server]` over a request run only under `server` without `standalone`, and its test of `haze::launch` only under `standalone` without `server`. `--all-features` reaches neither, so CI also tests each feature alone.

Locally:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p haze --all-targets --no-default-features -- -D warnings
cargo clippy -p haze --all-targets --features hooks -- -D warnings
cargo clippy -p haze --all-targets --features web -- -D warnings
cargo clippy -p haze --features server -- -D warnings
cargo clippy -p haze --all-targets --features server -- -D warnings
cargo clippy -p haze --all-targets --features standalone -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --no-default-features
cargo test -p haze --no-default-features
cargo test -p haze --features hooks
cargo test -p haze --features web
cargo test -p haze --features server
cargo test -p haze --features standalone
```

## License

[MIT](LICENSE)
