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
    Ok(greeters.iter().map(|greeter| greeter.greet()).collect())
}

#[component]
fn App() -> Element {
    let greetings = use_resource(greetings);
    let result = greetings.read();
    let lines = match &*result {
        Some(Ok(lines)) => lines.as_slice(),
        Some(Err(_)) | None => &[],
    };
    rsx! {
        for line in lines {
            p { {line.as_str()} }
        }
    }
}
