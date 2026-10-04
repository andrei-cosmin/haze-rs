//! The `Pack` trait: a struct of resources built once at startup.

use anyhow::Result;

use crate::Resources;

/// A struct of resources built once at startup and inserted as a resource.
///
/// [`#[derive(Pack)]`](derive@crate::Pack) implements this trait and submits
/// the struct for startup, which builds it after the resource functions and
/// inserts it under its own type. A server function takes a pack like any
/// other resource, as [`Res<Scores>`](struct@crate::Res), or as
/// `Option<Res<Scores>>` to receive `None` when the registry has none. This is
/// haze's counterpart to summer's `Service`, which is `Clone`, built once from
/// the registry and stored as a component.
///
/// A hand-written `impl Pack` only provides `build`, which runs where
/// `Pack::build` is called; startup never builds or inserts such a type, so a
/// [`#[register]`](macro@crate::register)ed one must be inserted, in setup or
/// by a [`#[resource]`](macro@crate::resource) function.
///
/// A pack does not implement [`Build`](crate::Build): a registered pack is
/// cloned from the registry once startup has built it, so it is built once and
/// the `Seq` holds a clone of it, sharing its `Arc` fields.
///
/// # Examples
///
/// ```rust,ignore
/// use haze::{Pack, Res};
///
/// #[derive(Clone, Pack)]
/// struct Scores {
///     store: Store,
///     motto: Option<Motto>,
/// }
///
/// #[server(scores: Res<Scores>)]
/// async fn best() -> Result<String> {
///     Ok(scores.store.best())
/// }
/// ```
pub trait Pack: Clone + Send + Sync + Sized + 'static {
    /// Builds the pack from `resources`, failing when a field's type is missing.
    fn build(resources: &Resources) -> Result<Self>;
}
