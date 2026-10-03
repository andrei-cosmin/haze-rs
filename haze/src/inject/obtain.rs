//! Picks `Build` over an inserted value at compile time for `#[haze::register]`.
//!
//! A registered type is obtained one of two ways: built with [`Build`] if it
//! implements it, otherwise cloned from [`Resources`]. Rust has no stable
//! specialization, so the choice is made with autoref dispatch, the same trick
//! `anyhow!` uses to pick between error kinds (anyhow's `kind.rs`).
//!
//! The generated code binds `Obtain::<T>(PhantomData)` and calls `haze_check`
//! and `haze_obtain` on a reference to it. Method lookup tries the receiver
//! `&Obtain<T>` before `&&Obtain<T>`:
//!
//! - [`Built`] is implemented for `Obtain<T>`, so its `&self` methods take
//!   `&Obtain<T>` and are found first whenever `T: Build`.
//! - [`Inserted`] is implemented for `&Obtain<T>`, so its `&self` methods take
//!   `&&Obtain<T>` and are only reached through one more auto-reference, when
//!   `T` does not implement `Build`.
//!
//! The methods carry the `haze_` prefix like anyhow's `anyhow_kind` and haze's
//! own `haze_kind`, so a user trait in scope with a `check` or `obtain` method
//! cannot take over the call. `haze_check` runs for every registration before
//! `Resources::collect` builds anything: a value to clone must already be
//! inserted, a `Build` type always passes. Checking every dependency before the
//! first build is the order of summer's `build_plugins`, and it keeps a trait
//! whose later implementation still waits for a pack from building its earlier
//! implementations again in the next round.

use std::marker::PhantomData;

use anyhow::Result;

use crate::{Build, Resources};

/// The dispatch target the generated code calls `haze_check` and `haze_obtain` on.
#[doc(hidden)]
#[derive(Debug)]
pub struct Obtain<T>(pub PhantomData<T>);

/// Obtains `T` by building it; preferred when `T: Build`.
#[doc(hidden)]
pub trait Built<T> {
    /// Builds `T` from `resources`.
    fn haze_obtain(&self, resources: &Resources) -> Result<T>;

    /// Nothing to check: `T` is built, not fetched.
    fn haze_check(&self, resources: &Resources) -> Result<()>;
}

/// Obtains `T` by cloning the inserted value; used when `T` has no `Build`.
#[doc(hidden)]
pub trait Inserted<T> {
    /// Clones the `T` inserted into `resources`.
    fn haze_obtain(&self, resources: &Resources) -> Result<T>;

    /// Fails unless a `T` is inserted into `resources`.
    fn haze_check(&self, resources: &Resources) -> Result<()>;
}

impl<T: Build> Built<T> for Obtain<T> {
    fn haze_obtain(&self, resources: &Resources) -> Result<T> {
        T::build(resources)
    }

    fn haze_check(&self, _resources: &Resources) -> Result<()> {
        Ok(())
    }
}

impl<T: Clone + Send + Sync + 'static> Inserted<T> for &Obtain<T> {
    fn haze_obtain(&self, resources: &Resources) -> Result<T> {
        resources.try_get::<T>()
    }

    fn haze_check(&self, resources: &Resources) -> Result<()> {
        resources.try_get::<T>().map(drop)
    }
}
