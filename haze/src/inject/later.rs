//! `Later<T>`: a resource filled by the end of startup, for cycles.

use std::{
    any::type_name,
    fmt::{self, Debug, Formatter},
    sync::{Arc, OnceLock},
};

use anyhow::{Result, anyhow};

/// A resource that becomes available by the end of startup, so resource
/// functions and packs can depend on each other in a cycle.
///
/// Take `Later<T>` instead of `T` in a [`#[resource]`](macro@crate::resource)
/// function, or as a [`#[derive(Pack)]`](derive@crate::Pack) field. It does not
/// count as a dependency, so the function or pack is built without waiting for
/// `T`. If `T` is already inserted, the `Later` holds it right away;
/// otherwise [`Resources::finish`](crate::Resources::finish) fills it when
/// startup ends, and fails startup if `T` was never inserted. Either way,
/// [`get`](Later::get) works from the first request on.
///
/// This is haze's counterpart to summer's `LazyComponent`, resolved at the end
/// of startup instead of through a global registry.
///
/// # Examples
///
/// ```rust,ignore
/// #[haze::resource]
/// fn cache(db: Later<Db>) -> Cache {
///     Cache::new(db)
/// }
///
/// #[haze::resource]
/// fn db(cache: Cache) -> Db {
///     Db::new(cache)
/// }
///
/// impl Cache {
///     fn lookup(&self, key: &str) -> Result<Row> {
///         let db = self.db.get()?;
///         db.row(key)
///     }
/// }
/// ```
#[derive(Clone)]
pub struct Later<T> {
    /// Shared with the pending fill in [`Resources`](crate::Resources) until it runs.
    value: Arc<OnceLock<T>>,
}

impl<T: Clone + Send + Sync + 'static> Later<T> {
    /// Wraps a cell that is already filled or will be filled by `finish`.
    pub(crate) fn new(value: Arc<OnceLock<T>>) -> Self {
        Self { value }
    }

    /// Gets the resource.
    ///
    /// # Errors
    ///
    /// Fails when called during startup, before `T` was inserted and
    /// [`Resources::finish`](crate::Resources::finish) ran.
    pub fn get(&self) -> Result<T> {
        self.value.get().cloned().ok_or_else(|| {
            anyhow!(
                "Later<{}> was used before startup finished",
                type_name::<T>()
            )
        })
    }

    /// Returns the resource, or `None` while it is not available yet.
    #[must_use]
    pub fn get_if_initialized(&self) -> Option<T> {
        self.value.get().cloned()
    }
}

impl<T: Clone + Send + Sync + 'static> Debug for Later<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Later")
            .field("initialized", &self.value.get().is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::Resources;

    #[test]
    fn get_waits_for_startup_then_reads_the_value() {
        let mut resources = Resources::new();
        let later = resources.later::<u64>();
        assert_eq!(
            later.get().unwrap_err().to_string(),
            "Later<u64> was used before startup finished"
        );
        resources.insert(7_u64);
        resources.finish().unwrap();
        assert_eq!(later.get().unwrap(), 7);
        assert_eq!(later.get_if_initialized(), Some(7));
    }

    #[test]
    fn a_type_already_inserted_is_filled_right_away() {
        let mut resources = Resources::new();
        resources.insert(3_u16);
        assert_eq!(resources.later::<u16>().get().unwrap(), 3);
    }

    #[test]
    fn finish_names_every_missing_type_once_in_order() {
        let resources = Resources::new();
        let _first = resources.later::<u64>();
        let _second = resources.later::<u32>();
        let _third = resources.later::<u64>();
        assert_eq!(
            resources.finish().unwrap_err().to_string(),
            "Later<u32> needs u32, which was never inserted; Later<u64> needs u64, which was never inserted"
        );
    }

    #[test]
    fn debug_shows_whether_it_was_filled() {
        let mut resources = Resources::new();
        let later = resources.later::<u8>();
        assert_eq!(format!("{later:?}"), "Later { initialized: false, .. }");
        resources.insert(1_u8);
        resources.finish().unwrap();
        assert_eq!(format!("{later:?}"), "Later { initialized: true, .. }");
    }

    #[test]
    fn finish_twice_does_nothing_more() {
        let mut resources = Resources::new();
        let later = resources.later::<u8>();
        resources.insert(4_u8);
        resources.finish().unwrap();
        resources.finish().unwrap();
        assert_eq!(later.get().unwrap(), 4);
    }

    #[test]
    fn later_after_finish_reads_a_present_type() {
        let mut resources = Resources::new();
        resources.insert(9_u32);
        resources.finish().unwrap();
        assert_eq!(resources.later::<u32>().get().unwrap(), 9);
    }

    #[test]
    #[should_panic(
        expected = "Later<u64> requested after startup finished, but u64 was never inserted"
    )]
    fn later_after_finish_for_a_missing_type_panics() {
        let resources = Resources::new();
        resources.finish().unwrap();
        let _later = resources.later::<u64>();
    }
}
