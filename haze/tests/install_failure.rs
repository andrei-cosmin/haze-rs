#![cfg(feature = "standalone")]

use anyhow::anyhow;
use haze::Resources;

#[test]
#[should_panic(expected = "opening data.redb\n\nCaused by:\n    database is locked")]
fn a_failing_setup_panics_with_the_error_and_its_causes() {
    let _resources = haze::install(async |_: &mut Resources| {
        Err(anyhow!("database is locked").context("opening data.redb"))
    });
}
