//! The `Build` trait: building a value from `Resources`.

use anyhow::Result;

use crate::Resources;

/// Builds a value from server resources.
///
/// Implement it for a [`#[register]`](macro@crate::register)ed type that is
/// neither inserted, in setup or by a [`#[resource]`](macro@crate::resource)
/// function, nor a [`Pack`](trait@crate::Pack).
/// [`Resources::collect`](crate::Resources::collect) uses it for every
/// registered implementation that implements it; the others, inserted values
/// and packs, are cloned from the registry.
///
/// Startup may call `build` more than once: once for each trait the type is
/// registered for, so each `Seq` holds its own instance, and again when a
/// `Build` implementation of the same trait fails because what it needs
/// arrives in a later round, which collects that trait again. To share state
/// between traits, do not implement `Build`: derive `Pack`, or insert the value
/// in setup or from a [`#[resource]`](macro@crate::resource) function; each
/// `Seq` then holds a clone that shares its `Arc` fields. Keep side effects
/// such as spawning a task out of `build`; a pack's `#[pack(func = ..)]` field
/// runs once.
///
/// # Examples
///
/// ```rust,ignore
/// use std::sync::Arc;
///
/// use anyhow::Result;
/// use haze::{Build, Resources};
///
/// impl Build for Recorder {
///     fn build(resources: &Resources) -> Result<Self> {
///         let log = resources.try_get::<Arc<EntryLog>>()?;
///         Ok(Self { log, limit: 10_000 })
///     }
/// }
/// ```
pub trait Build: Sized {
    /// Builds `Self` from `resources`, failing when something it needs is missing.
    fn build(resources: &Resources) -> Result<Self>;
}
