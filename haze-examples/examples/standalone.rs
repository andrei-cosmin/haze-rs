#![allow(clippy::unused_async)]

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use dioxus::prelude::*;
use haze::Pack;

fn main() {
    haze::launch(
        async |resources| {
            resources.insert(Clicks::default());
            resources.insert(Title(String::from("Clicks")));
            Ok(())
        },
        App,
    );
}

#[derive(Clone, Default)]
struct Clicks(Arc<AtomicU64>);

impl Clicks {
    fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
struct Title(String);

#[derive(Clone, Pack)]
struct Counter {
    clicks: Clicks,
    title: Title,
}

impl Counter {
    fn describe(&self) -> String {
        format!("{}: {}", self.title.0, self.clicks.get())
    }
}

#[haze::server(clicks: Clicks)]
async fn bump() -> Result<u64> {
    Ok(clicks.bump())
}

#[haze::server(counter: Counter)]
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
