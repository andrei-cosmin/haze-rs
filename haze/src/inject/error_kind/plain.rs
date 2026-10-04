//! The tag that passes every other error through to `anyhow!` unchanged.

/// The tag for every other error type.
#[doc(hidden)]
#[derive(Debug)]
pub struct Plain;

/// Picks [`Plain`] for any other error.
#[doc(hidden)]
pub trait PlainKind {
    /// Returns the tag for any other error.
    #[inline]
    fn haze_kind(&self) -> Plain {
        Plain
    }
}

impl<E> PlainKind for &E {}

impl Plain {
    /// Returns the error unchanged for `anyhow!` to convert.
    #[cold]
    #[must_use]
    #[allow(clippy::unused_self)]
    pub fn prepare<E>(self, error: E) -> E {
        error
    }
}
