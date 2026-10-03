#![allow(clippy::unused_async)]

use dioxus::prelude::*;

#[cfg(feature = "server")]
use anyhow::Error;
#[cfg(feature = "server")]
use haze::{Build, Pack, Res, Resources, Seq};

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    haze::serve(
        async |resources| {
            resources.insert(Name(String::from("Ana")));
            Ok(())
        },
        || dioxus::server::router(App),
    );
}

#[cfg(feature = "server")]
trait Greeter: Send + Sync {
    fn greet(&self) -> String;
}

#[cfg(feature = "server")]
#[derive(Clone)]
struct Name(String);

#[cfg(feature = "server")]
#[derive(Clone, Pack)]
struct Personal {
    name: Name,
}

#[cfg(feature = "server")]
#[haze::register(order = 10)]
impl Greeter for Personal {
    fn greet(&self) -> String {
        format!("Hello, {}!", self.name.0)
    }
}

#[cfg(feature = "server")]
struct Generic;

#[cfg(feature = "server")]
impl Build for Generic {
    fn build(_resources: &Resources) -> Result<Self, Error> {
        Ok(Self)
    }
}

#[cfg(feature = "server")]
#[haze::register(order = 20)]
impl Greeter for Generic {
    fn greet(&self) -> String {
        String::from("Welcome back.")
    }
}

#[server(greeters: Res<Seq<dyn Greeter>>)]
async fn greetings() -> Result<Vec<String>> {
    let mut greetings = Vec::new();
    for greeter in greeters.iter() {
        greetings.push(greeter.greet());
    }
    Ok(greetings)
}

#[component]
fn App() -> Element {
    let greetings = use_resource(greetings);
    let lines = greetings().and_then(Result::ok).unwrap_or_default();
    rsx! {
        for line in lines {
            p { "{line}" }
        }
    }
}
