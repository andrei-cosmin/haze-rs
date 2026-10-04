#![cfg(feature = "standalone")]

use std::sync::atomic::{AtomicUsize, Ordering};

use haze::Resources;

#[derive(Debug, PartialEq)]
struct Season(&'static str);

#[test]
fn a_second_install_returns_the_first_registry_and_runs_no_setup() {
    let runs = AtomicUsize::new(0);
    let first = haze::install(async |resources: &mut Resources| {
        runs.fetch_add(1, Ordering::Relaxed);
        resources.insert(Season("spring"));
        Ok(())
    });
    let second = haze::install(async |resources: &mut Resources| {
        runs.fetch_add(1, Ordering::Relaxed);
        resources.insert(Season("winter"));
        Ok(())
    });
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    assert_eq!(first.get::<Season>(), Some(&Season("spring")));
    assert_eq!(second.get::<Season>(), Some(&Season("spring")));
}
