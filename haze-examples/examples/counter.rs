#![allow(clippy::unused_async)]

use dioxus::prelude::*;

#[cfg(feature = "server")]
use {
    haze::{Pack, Res},
    std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    haze::serve(
        async |resources| {
            resources.insert(Clicks::default());
            resources.insert(Title(String::from("Clicks")));
            Ok(())
        },
        || dioxus::server::router(App),
    );
}

#[cfg(feature = "server")]
#[derive(Clone, Default)]
struct Clicks(Arc<AtomicU64>);

#[cfg(feature = "server")]
impl Clicks {
    fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(feature = "server")]
#[derive(Clone)]
struct Title(String);

#[cfg(feature = "server")]
#[derive(Clone, Pack)]
struct Counter {
    clicks: Clicks,
    title: Title,
}

#[cfg(feature = "server")]
impl Counter {
    fn describe(&self) -> String {
        format!("{}: {}", self.title.0, self.clicks.get())
    }
}

#[server(clicks: Res<Clicks>)]
async fn bump() -> Result<u64> {
    Ok(clicks.bump())
}

#[server(counter: Res<Counter>)]
async fn summary() -> Result<String> {
    Ok(counter.describe())
}

#[component]
fn App() -> Element {
    let mut text = use_resource(summary);
    let label = match &*text.read() {
        Some(Ok(value)) => value.clone(),
        Some(Err(error)) => error.to_string(),
        None => String::from("Loading"),
    };
    rsx! {
        h1 { "{label}" }
        button {
            onclick: move |_| async move {
                _ = bump().await;
                text.restart();
            },
            "Add one"
        }
    }
}
