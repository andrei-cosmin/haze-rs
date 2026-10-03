#![allow(clippy::unused_async)]

use dioxus::{fullstack::ServerEvents, prelude::*};

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
            resources.insert(Topic(String::from("Release")));
            resources.insert(Pace(Duration::from_secs(1)));
            Ok(())
        },
        || dioxus::server::router(App),
    );
}

#[cfg(feature = "server")]
#[derive(Clone)]
struct Topic(String);

#[cfg(feature = "server")]
#[derive(Clone)]
struct Pace(Duration);

#[cfg(feature = "server")]
#[derive(Clone, Pack)]
struct Announcer {
    topic: Topic,
    pace: Pace,
}

#[get("/api/announcements", announcer: Res<Announcer>)]
async fn announcements() -> Result<ServerEvents<String>> {
    Ok(ServerEvents::new(move |mut sender| async move {
        for step in 1_u64.. {
            let event = format!("{} step {step}", announcer.topic.0);
            if sender.send(event).await.is_err() {
                return;
            }
            tokio::time::sleep(announcer.pace.0).await;
        }
    }))
}

#[component]
fn App() -> Element {
    let mut events = use_signal(Vec::<String>::new);
    let status = haze::use_server_events(announcements, move |event| {
        let mut shown = events.write();
        shown.push(event);
        if shown.len() > 5 {
            shown.remove(0);
        }
    });
    rsx! {
        p { "{status:?}" }
        for event in events.read().iter() {
            p { "{event}" }
        }
    }
}
