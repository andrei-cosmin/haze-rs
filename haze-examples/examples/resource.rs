#![allow(clippy::unused_async)]

use dioxus::prelude::*;

#[cfg(feature = "server")]
use {
    haze::{Later, Res},
    std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    haze::serve(async |_| Ok(()), || dioxus::server::router(App));
}

#[cfg(feature = "server")]
#[derive(Clone, Default)]
struct Visits(Arc<AtomicU64>);

#[cfg(feature = "server")]
#[derive(Clone)]
struct Motto(String);

#[cfg(feature = "server")]
#[derive(Clone)]
struct Greeter {
    visits: Visits,
    motto: Option<Motto>,
    door: &'static str,
}

#[cfg(feature = "server")]
#[derive(Clone)]
struct Door {
    label: &'static str,
    greeter: Later<Greeter>,
}

#[cfg(feature = "server")]
#[haze::resource]
fn visits() -> Visits {
    Visits::default()
}

#[cfg(feature = "server")]
#[haze::resource]
fn greeter(visits: Visits, motto: Option<Motto>, door: Door) -> Greeter {
    Greeter {
        visits,
        motto,
        door: door.label,
    }
}

#[cfg(feature = "server")]
#[haze::resource]
fn door(greeter: Later<Greeter>) -> Door {
    Door {
        label: "front door",
        greeter,
    }
}

#[cfg(feature = "server")]
impl Door {
    fn enter(&self) -> Result<String> {
        let greeter = self.greeter.get()?;
        let visit = greeter.visits.0.fetch_add(1, Ordering::Relaxed) + 1;
        let motto = greeter
            .motto
            .map_or(String::from("no motto"), |motto| motto.0);
        Ok(format!(
            "Welcome, visitor {visit}, at the {} ({motto})",
            greeter.door
        ))
    }
}

#[server(door: Res<Door>)]
async fn enter() -> Result<String> {
    door.enter()
}

#[component]
fn App() -> Element {
    let mut greeting = use_signal(String::new);
    rsx! {
        button {
            onclick: move |_| async move {
                if let Ok(text) = enter().await {
                    greeting.set(text);
                }
            },
            "Enter"
        }
        p { "{greeting}" }
    }
}
