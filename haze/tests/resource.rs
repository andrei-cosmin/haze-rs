use std::sync::Arc;

use anyhow::{Result, anyhow};
use dioxus::CapturedError;
use haze::{Later, Resources, Seq};

#[derive(Clone)]
struct Store(String);

#[derive(Clone)]
struct Theme(&'static str);

#[haze::resource]
#[allow(clippy::unnecessary_wraps)]
fn theme() -> Result<Theme, CapturedError> {
    Ok(Theme("dark"))
}

#[derive(Clone)]
struct Greeting(String);

#[derive(Clone)]
struct Farewell(String);

#[haze::resource]
fn greeting(farewell: Option<Farewell>) -> Greeting {
    let farewell = farewell.map_or(String::from("none"), |farewell| farewell.0);
    Greeting(format!("hello, then {farewell}"))
}

trait Speaker: Send + Sync {
    fn speak(&self) -> String;
}

#[derive(Clone, haze::Pack)]
struct Announcer {
    greeting: Greeting,
}

#[haze::register(order = 10)]
impl Speaker for Announcer {
    fn speak(&self) -> String {
        self.greeting.0.clone()
    }
}

#[haze::resource]
async fn store() -> Result<Arc<Store>> {
    tokio::task::yield_now().await;
    Ok(Arc::new(Store(String::from("disk"))))
}

#[haze::resource]
fn farewell(store: Arc<Store>) -> Farewell {
    Farewell(format!("bye from {}", store.0))
}

#[derive(Clone)]
struct Named(&'static str);

#[derive(Clone)]
struct Echo(&'static str);

#[haze::resource]
fn provide() -> Named {
    Named("provide")
}

#[haze::resource]
fn resources(named: Named) -> Echo {
    Echo(named.0)
}

#[derive(Clone)]
struct Cache {
    db: Later<Db>,
}

#[derive(Clone)]
struct Db {
    cache: Cache,
    name: &'static str,
}

#[haze::resource]
fn cache(db: Later<Db>) -> Cache {
    Cache { db }
}

#[haze::resource]
fn db(cache: Cache) -> Db {
    Db {
        cache,
        name: "main",
    }
}

#[tokio::test]
async fn a_function_returning_a_dioxus_result_is_provided() {
    let mut resources = Resources::new();
    resources.provide().await.unwrap();
    assert_eq!(resources.get::<Theme>().unwrap().0, "dark");
}

#[tokio::test]
async fn resource_functions_provide_each_other_in_any_order() {
    let mut resources = Resources::new();
    resources.provide().await.unwrap();
    assert_eq!(
        resources.get::<Greeting>().unwrap().0,
        "hello, then bye from disk"
    );
}

#[tokio::test]
async fn a_resource_inserted_by_hand_replaces_its_function() {
    let mut resources = Resources::new();
    resources.insert(Arc::new(Store(String::from("memory"))));
    resources.provide().await.unwrap();
    assert_eq!(
        resources.get::<Greeting>().unwrap().0,
        "hello, then bye from memory"
    );
}

#[tokio::test]
async fn start_collects_registered_implementations_after_resource_functions() {
    let resources = Resources::start(async |_| Ok(())).await.unwrap();
    let speakers = resources.get::<Seq<dyn Speaker>>().unwrap();
    assert_eq!(speakers[0].speak(), "hello, then bye from disk");
}

#[tokio::test]
async fn function_names_do_not_clash_with_generated_code() {
    let mut registry = Resources::new();
    registry.provide().await.unwrap();
    assert_eq!(registry.get::<Echo>().unwrap().0, "provide");
}

#[tokio::test]
async fn later_breaks_a_cycle_and_resolves_after_startup() {
    let mut registry = Resources::new();
    registry.provide().await.unwrap();
    let cache = registry.get::<Cache>().unwrap();
    let Err(early) = cache.db.get() else {
        panic!("Later resolved before startup finished");
    };
    assert_eq!(
        early.to_string(),
        "Later<resource::Db> was used before startup finished"
    );
    registry.finish().unwrap();
    let Ok(db) = cache.db.get() else {
        panic!("Later did not resolve after startup finished");
    };
    assert_eq!(db.name, "main");
    assert!(db.cache.db.get_if_initialized().is_some());
}

#[tokio::test]
async fn start_runs_setup_then_resource_functions_then_fills_later() {
    let resources = Resources::start(async |resources| {
        resources.insert(Arc::new(Store(String::from("setup"))));
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        resources.get::<Greeting>().unwrap().0,
        "hello, then bye from setup"
    );
    let cache = resources.get::<Cache>().unwrap();
    assert_eq!(cache.db.get().unwrap().name, "main");
}

#[tokio::test]
async fn start_stops_at_a_failing_setup() {
    let Err(error) = Resources::start(async |_| Err(anyhow!("no disk"))).await else {
        panic!("start succeeded although setup failed");
    };
    assert_eq!(error.to_string(), "no disk");
}

#[tokio::test]
async fn start_stops_at_a_later_nobody_fills() {
    let Err(error) = Resources::start(async |resources| {
        let _unfilled = resources.later::<Store>();
        Ok(())
    })
    .await
    else {
        panic!("start succeeded although a Later could not be filled");
    };
    assert_eq!(
        error.to_string(),
        "Later<resource::Store> needs resource::Store, which was never inserted"
    );
}

#[derive(Clone)]
struct Label(&'static str);

#[haze::resource]
fn label(tag: Option<&'static str>) -> Label {
    Label(tag.unwrap_or("untagged"))
}

#[tokio::test]
async fn a_static_reference_is_a_resource_like_any_other() {
    let resources = Resources::start(async |resources| {
        resources.insert("season one");
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(resources.get::<Label>().unwrap().0, "season one");
}
