use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, bail};
use haze::{Build, Resources, Seq};

trait Probe {
    fn check(&self) -> bool {
        true
    }

    fn obtain(&self) -> u8 {
        0
    }
}

impl<T> Probe for T {}

trait Stage: Send + Sync {}

struct Broken;

impl Build for Broken {
    fn build(_resources: &Resources) -> Result<Self> {
        bail!("the stage is broken")
    }
}

#[haze::register(order = 1)]
impl Stage for Broken {}

#[derive(Clone)]
struct Absent;

#[haze::register(order = 2)]
impl Stage for Absent {}

trait Mark: Send + Sync {
    fn mark(&self) -> &'static str;
}

#[derive(Clone)]
struct Both(&'static str);

impl Build for Both {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self("built"))
    }
}

#[haze::register(order = 1)]
impl Mark for Both {
    fn mark(&self) -> &'static str {
        self.0
    }
}

trait Reader: Send + Sync {
    fn reads(&self) -> u64;
}

trait Writer: Send + Sync {
    fn write(&self);
}

#[derive(Clone, Default)]
struct Builds(Arc<AtomicU64>);

struct Disk {
    writes: AtomicU64,
}

impl Build for Disk {
    fn build(resources: &Resources) -> Result<Self> {
        resources
            .try_get::<Builds>()?
            .0
            .fetch_add(1, Ordering::Relaxed);
        Ok(Self {
            writes: AtomicU64::new(0),
        })
    }
}

#[haze::register(order = 1)]
impl Reader for Disk {
    fn reads(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }
}

#[haze::register(order = 1)]
impl Writer for Disk {
    fn write(&self) {
        self.writes.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn an_implementation_to_clone_is_checked_before_anything_is_built() {
    let error = Resources::new().collect::<dyn Stage>().unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "collecting register_check::Absent as dyn register_check::Stage: register_check::Absent was never inserted"
    );
}

#[test]
fn a_failing_build_is_named_once_every_implementation_to_clone_is_present() {
    let mut resources = Resources::new();
    resources.insert(Absent);
    let error = resources.collect::<dyn Stage>().unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "collecting register_check::Broken as dyn register_check::Stage: the stage is broken"
    );
}

#[test]
fn a_build_type_is_built_even_when_nothing_is_inserted() {
    let mut resources = Resources::new();
    resources.collect::<dyn Mark>().unwrap();
    assert_eq!(resources.get::<Seq<dyn Mark>>().unwrap()[0].mark(), "built");
}

#[test]
fn build_takes_precedence_over_an_inserted_value() {
    let mut resources = Resources::new();
    resources.insert(Both("inserted"));
    resources.collect::<dyn Mark>().unwrap();
    assert_eq!(resources.get::<Seq<dyn Mark>>().unwrap()[0].mark(), "built");
}

#[test]
fn a_trait_in_scope_with_check_and_obtain_methods_leaves_registration_alone() {
    assert!(Absent.check());
    assert_eq!(Absent.obtain(), 0);
}

#[test]
fn a_build_type_registered_for_two_traits_is_built_once_per_trait() {
    let mut resources = Resources::new();
    resources.insert(Builds::default());
    resources.collect::<dyn Reader>().unwrap();
    resources.collect::<dyn Writer>().unwrap();
    resources.get::<Seq<dyn Writer>>().unwrap()[0].write();
    assert_eq!(resources.get::<Seq<dyn Reader>>().unwrap()[0].reads(), 0);
    assert_eq!(
        resources.get::<Builds>().unwrap().0.load(Ordering::Relaxed),
        2
    );
}
