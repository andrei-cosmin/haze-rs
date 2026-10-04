//! The tag that recovers a Dioxus [`CapturedError`]'s inner `anyhow::Error`.

use std::sync::Arc;

use dioxus::CapturedError;

/// The tag for a Dioxus [`CapturedError`].
#[doc(hidden)]
#[derive(Debug)]
pub struct Captured;

/// Picks [`Captured`] for a `CapturedError`.
#[doc(hidden)]
pub trait CapturedKind {
    /// Returns the tag for a `CapturedError`.
    #[inline]
    fn haze_kind(&self) -> Captured {
        Captured
    }
}

impl CapturedKind for CapturedError {}

impl Captured {
    /// Recovers the inner `anyhow::Error`, or keeps the full chain as text when
    /// another clone of the error is still alive.
    #[cold]
    #[must_use]
    #[allow(clippy::unused_self)]
    pub fn prepare(self, error: CapturedError) -> anyhow::Error {
        Arc::try_unwrap(error.0).unwrap_or_else(|shared| anyhow::Error::msg(format!("{shared:#}")))
    }
}
