#![cfg(all(feature = "standalone", not(feature = "server")))]

use std::{any::Any, sync::OnceLock};

use dioxus::core::{Element, VNode};
use dioxus::{ContextFn, LaunchBuilder};
use haze::Resources;
use tokio::runtime::{Handle, RuntimeFlavor};

#[derive(Debug, PartialEq)]
struct Season(&'static str);

struct Seen {
    flavor: Option<RuntimeFlavor>,
    season: Option<&'static Season>,
}

impl Seen {
    fn record(_app: fn() -> Element, _contexts: Vec<ContextFn>, _configs: Vec<Box<dyn Any>>) {
        let seen = Self {
            flavor: Handle::try_current()
                .ok()
                .map(|handle| handle.runtime_flavor()),
            season: Resources::get_default().unwrap().get::<Season>(),
        };
        assert!(SEEN.set(seen).is_ok(), "the renderer is launched once");
    }
}

static SEEN: OnceLock<Seen> = OnceLock::new();

fn app() -> Element {
    VNode::empty()
}

#[test]
fn launch_installs_the_registry_and_enters_the_runtime_before_handing_the_app_to_dioxus() {
    haze::launch(
        async |resources| {
            resources.insert(Season("spring"));
            Ok(())
        },
        LaunchBuilder::custom(Seen::record),
        app,
    );

    let seen = SEEN.get().expect("dioxus launched the renderer");
    assert_eq!(seen.flavor, Some(RuntimeFlavor::MultiThread));
    assert_eq!(seen.season, Some(&Season("spring")));
}
