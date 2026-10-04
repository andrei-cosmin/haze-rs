#![cfg(feature = "hooks")]

use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use dioxus::CapturedError;
use haze::Resources;

#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<CapturedError>>>);

#[derive(Clone)]
struct Theme;

#[haze::resource]
fn theme(journal: Journal) -> Result<Theme, CapturedError> {
    let Journal(entries) = journal;
    let cause = anyhow!("disk is full").context("opening the theme file");
    let error = CapturedError(Arc::new(cause));
    entries.lock().unwrap().push(error.clone());
    Err(error)
}

#[tokio::test]
async fn a_shared_dioxus_error_keeps_its_causes_as_text() {
    let journal = Journal::default();
    let mut resources = Resources::new();
    resources.insert(journal.clone());
    let error = resources.provide().await.unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "shared_failing_resource::theme failed to provide shared_failing_resource::Theme: opening the theme file: disk is full"
    );
    assert_eq!(error.chain().count(), 2);
    assert_eq!(journal.0.lock().unwrap().len(), 1);
}
