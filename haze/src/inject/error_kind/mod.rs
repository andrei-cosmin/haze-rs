//! Keeps a Dioxus error's causes when a `#[haze::resource]` function fails.
//!
//! Dioxus's `Result` carries a [`CapturedError`](dioxus::CapturedError), an
//! `Arc<anyhow::Error>` that is not a `std::error::Error`, so `anyhow!` would
//! wrap it as a message, keeping only its outermost text and losing its causes.
//! The generated code first calls
//! `(&error).haze_kind().prepare(error)`, the same tagged dispatch `anyhow!`
//! itself uses in its `kind.rs`:
//!
//! - [`CapturedKind`] exists with the `hooks` feature, which brings in Dioxus,
//!   and is implemented for `CapturedError`, so its `&self` method takes
//!   `&CapturedError` and wins for that type; it hands back the inner
//!   `anyhow::Error` with its whole chain. The generated code glob-imports
//!   `__private::error_kind`, the way `anyhow!` glob-imports its
//!   `__private::kind`, so without `hooks` the trait is absent and a
//!   `CapturedError` takes the plain path, where `anyhow!` keeps only its
//!   outermost message.
//! - [`PlainKind`] is implemented for `&E`, so it is reached only through one
//!   more auto-reference for every other error, which it returns unchanged for
//!   `anyhow!` to convert.

#[cfg(feature = "hooks")]
mod captured;
mod plain;

#[cfg(feature = "hooks")]
pub use captured::CapturedKind;
pub use plain::PlainKind;
