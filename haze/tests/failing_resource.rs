use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use dioxus::CapturedError;
use haze::Resources;

#[derive(Clone)]
struct Theme;

#[derive(Clone)]
struct Palette;

#[haze::resource]
fn theme() -> Result<Theme, CapturedError> {
    let cause = anyhow!("disk is full").context("opening the theme file");
    Err(CapturedError(Arc::new(cause)))
}

#[haze::resource]
fn palette() -> Result<Palette> {
    Err(anyhow!("disk is full")).context("reading the palette file")
}

#[cfg(feature = "hooks")]
#[tokio::test]
async fn a_dioxus_error_keeps_its_causes_in_the_startup_error() {
    let mut resources = Resources::new();
    resources.insert(Palette);
    let error = resources.provide().await.unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "failing_resource::theme failed to provide failing_resource::Theme: opening the theme file: disk is full"
    );
    assert_eq!(error.chain().count(), 3);
}

#[cfg(not(feature = "hooks"))]
#[tokio::test]
async fn without_hooks_a_dioxus_error_keeps_only_its_outermost_message() {
    let mut resources = Resources::new();
    resources.insert(Palette);
    let error = resources.provide().await.unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "failing_resource::theme failed to provide failing_resource::Theme: opening the theme file"
    );
    assert_eq!(error.chain().count(), 2);
}

#[tokio::test]
async fn an_anyhow_error_keeps_its_causes_in_the_startup_error() {
    let mut resources = Resources::new();
    resources.insert(Theme);
    let error = resources.provide().await.unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "failing_resource::palette failed to provide failing_resource::Palette: reading the palette file: disk is full"
    );
}
