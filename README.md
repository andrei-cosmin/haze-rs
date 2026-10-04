# haze

[![CI](https://github.com/andrei-cosmin/haze-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/andrei-cosmin/haze-rs/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Typed resources and server functions for [Dioxus](https://dioxuslabs.com), for fullstack and standalone apps.

## Features

- **Resources**: a type-keyed registry filled once at startup and injected into server functions.
- **Resource functions**: `#[haze::resource]` functions run at startup in dependency order.
- **Packs**: `#[derive(Pack)]` groups several resources into one handle.
- **Registration**: `#[haze::register]` collects trait implementations into an ordered `Seq<T>`; `#[haze::collect]` declares the trait, so the `Seq` exists when nothing registers, and `#[haze::collect(N)]` requires exactly `N` implementations.
- **Standalone mode**: the same `#[haze::server]` functions run in process, without a server.
- **Live hooks**: `use_streaming`, `use_server_events` and `use_websocket` reconnect automatically; their `_with` forms take `Reconnect` options for an idle timeout and jitter.

## Installation

Fullstack:

```toml
[dependencies]
dioxus = { version = "0.7.10", features = ["fullstack"] }
dioxus-fullstack = { version = "0.7.10", default-features = false }
haze = "0.8"

[features]
web = ["dioxus/web"]
server = ["dioxus/server", "haze/server"]
```

Standalone:

```toml
[dependencies]
dioxus = { version = "0.7.10", features = ["desktop"] }
haze = "0.8"

[features]
default = ["standalone"]
standalone = ["haze/standalone"]
```

## Example

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
    Ok(clicks.0.0.fetch_add(1, Ordering::Relaxed) + 1)
}

#[component]
fn App() -> Element {
    rsx! { button { onclick: move |_| async move { _ = bump().await; }, "+1" } }
}
```

Run with `dx serve --web --fullstack`. More examples are in [`haze-examples/examples`](haze-examples/examples).

## Feature flags

| Flag | Enables |
|---|---|
| *(none)* | `Resources`, `Res<T>`, resource functions, packs, registration, `#[haze::server]`, the client hooks; the browser transport is picked by the wasm target |
| `server` | `haze::serve`, `haze::client_rendered`, `Resources::current`, `Res<T>` as an axum extractor |
| `standalone` | `haze::launch`, `haze::install` on native targets, in-process server functions |

`server` and `standalone` must not be enabled in the same build.

## Documentation

The full guide, including feature crates, startup rules and standalone details, is in the crate documentation:

```sh
cargo doc -p haze --all-features --open
```

## License

[MIT](LICENSE)
