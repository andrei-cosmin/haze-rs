#![cfg(all(feature = "standalone", not(feature = "server")))]

use std::{
    panic,
    sync::{Arc, Mutex},
};

use dioxus::core::{Element, VNode};
use haze::Resources;
use tokio::runtime::{Handle, RuntimeFlavor};

#[derive(Clone, Debug, PartialEq)]
struct Season(&'static str);

fn app() -> Element {
    VNode::empty()
}

#[test]
fn launch_installs_the_registry_and_enters_the_runtime_before_handing_the_app_to_dioxus() {
    let flavor: Arc<Mutex<Option<RuntimeFlavor>>> = Arc::new(Mutex::new(None));
    let previous = panic::take_hook();
    {
        let flavor = Arc::clone(&flavor);
        panic::set_hook(Box::new(move |_| {
            *flavor.lock().unwrap() = Handle::try_current()
                .ok()
                .map(|handle| handle.runtime_flavor());
        }));
    }
    let launched = panic::catch_unwind(|| {
        haze::launch(
            async |resources| {
                resources.insert(Season("spring"));
                Ok(())
            },
            app,
        );
    });
    panic::set_hook(previous);
    let refusal = launched.unwrap_err();
    assert!(
        refusal
            .downcast_ref::<&str>()
            .unwrap()
            .starts_with("No platform feature enabled")
    );
    assert_eq!(*flavor.lock().unwrap(), Some(RuntimeFlavor::MultiThread));
    assert_eq!(
        Resources::get_default().unwrap().get::<Season>(),
        Some(Season("spring"))
    );
}
