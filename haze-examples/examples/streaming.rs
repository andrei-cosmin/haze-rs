#![allow(clippy::unused_async)]

use dioxus::{
    fullstack::{JsonEncoding, Streaming},
    prelude::*,
};

#[cfg(feature = "server")]
use {
    haze::{Pack, Res},
    std::time::Duration,
};

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    haze::serve(
        async |resources| {
            resources.insert(Start(10));
            resources.insert(Pace(Duration::from_millis(500)));
            Ok(())
        },
        || dioxus::server::router(App),
    );
}

#[cfg(feature = "server")]
#[derive(Clone)]
struct Start(u32);

#[cfg(feature = "server")]
#[derive(Clone)]
struct Pace(Duration);

#[cfg(feature = "server")]
#[derive(Clone, Pack)]
struct Countdown {
    start: Start,
    pace: Pace,
}

#[get("/api/countdown", countdown: Res<Countdown>)]
async fn countdown() -> Result<Streaming<u32, JsonEncoding>> {
    Ok(Streaming::spawn(move |sender| async move {
        loop {
            for value in (0..=countdown.start.0).rev() {
                if sender.unbounded_send(value).is_err() {
                    return;
                }
                tokio::time::sleep(countdown.pace.0).await;
            }
        }
    }))
}

#[component]
fn App() -> Element {
    let mut value = use_signal(|| None::<u32>);
    let status = haze::use_streaming(countdown, move |next| value.set(Some(next)));
    rsx! {
        h1 { "{value:?}" }
        p { "{status:?}" }
    }
}
