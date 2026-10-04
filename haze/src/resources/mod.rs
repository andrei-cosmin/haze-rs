//! The typed registry shared by every request, and ordered trait collections.

mod id_hasher;
mod seq;

pub use seq::Seq;

use std::{
    any::{Any, TypeId, type_name},
    collections::{HashMap, hash_map::Entry},
    fmt::{self, Debug, Formatter},
    hash::BuildHasherDefault,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(feature = "server")]
use dioxus_fullstack::{FullstackContext, HttpError, http::StatusCode};

use crate::inject::{Later, installer::Installer, provider::Provider, registration::Registration};
use id_hasher::IdHasher;

/// One inserted value with its type name, kept for `Debug`.
type Slot = (&'static str, Box<dyn Any + Send + Sync>);

/// Fills one handed-out [`Later`] from the finished registry.
type Fill = Box<dyn FnOnce(&Resources) -> Result<()> + Send>;

/// The registry [`Resources::install_default`] makes the default for this process.
static PROCESS_DEFAULT_RESOURCES: OnceLock<Resources> = OnceLock::new();

/// Resources, keyed by their exact type.
///
/// A registry is filled once at startup, by a setup closure, the
/// [`#[resource]`](macro@crate::resource) functions, the
/// [`#[register]`](macro@crate::register)ed implementations and the
/// [`#[derive(Pack)]`](derive@crate::Pack) structs; [`start`](Self::start)
/// runs those steps. Then it is shared one of two ways:
///
/// - In a fullstack app, [`serve`](fn@crate::serve) attaches it to every
///   request, where [`Res`](struct@crate::Res) reads from it in any handler
///   and [`current`](Self::current) inside a server function or server render.
/// - In a standalone app, [`install_default`](Self::install_default) makes it
///   the process default, which [`get_default`](Self::get_default) returns
///   anywhere in the process; [`launch`](fn@crate::launch) does both.
///
/// Lookup follows the rule of axum's `Extension`: a value is
/// found only under the exact type it was inserted as, and reading returns a
/// clone. Wrap values that are not `Clone`, or that must be shared rather than
/// copied, in an [`Arc`].
///
/// Cloning a `Resources` is cheap: clones share one map.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use haze::Resources;
///
/// let mut resources = Resources::new();
/// resources.insert(Arc::new(String::from("data.redb")));
///
/// assert!(resources.get::<Arc<String>>().is_some());
/// assert!(resources.get::<String>().is_none());
/// ```
#[derive(Clone)]
#[must_use]
pub struct Resources {
    /// The values, keyed by the exact type they were inserted as.
    slots: Arc<HashMap<TypeId, Slot, BuildHasherDefault<IdHasher>>>,
    /// Fills for the [`Later`]s handed out before their type existed, or `None`
    /// once [`finish`](Self::finish) ran, so both are decided under one lock.
    pending: Arc<Mutex<Option<Vec<Fill>>>>,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            slots: Arc::default(),
            pending: Arc::new(Mutex::new(Some(Vec::new()))),
        }
    }
}

impl Resources {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `value` under its type `T`.
    ///
    /// # Panics
    ///
    /// Panics if a value of type `T` was already inserted or another
    /// `Resources` clone still shares this registry. Insert everything
    /// during setup.
    #[track_caller]
    pub fn insert<T: Clone + Send + Sync + 'static>(&mut self, value: T) {
        let Some(slots) = Arc::get_mut(&mut self.slots) else {
            panic!(
                "{} inserted while Resources is shared; insert everything during setup",
                type_name::<T>()
            );
        };
        match slots.entry(TypeId::of::<T>()) {
            Entry::Vacant(slot) => {
                slot.insert((type_name::<T>(), Box::new(value)));
            }
            Entry::Occupied(_) => panic!("{} inserted twice", type_name::<T>()),
        }
    }

    /// Returns a clone of the value inserted as `T`, or `None` if there is none.
    #[must_use]
    pub fn get<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        let (_, value) = self.slots.get(&TypeId::of::<T>())?;
        value.downcast_ref::<T>().cloned()
    }

    /// Returns a clone of the value inserted as `T`.
    ///
    /// # Errors
    ///
    /// Fails when no value of type `T` was inserted, naming the type.
    pub fn try_get<T: Clone + Send + Sync + 'static>(&self) -> Result<T> {
        self.get::<T>()
            .ok_or_else(|| anyhow!("{} was never inserted", type_name::<T>()))
    }

    /// Returns whether a value of exactly type `T` was inserted.
    #[must_use]
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.contains_type_id(TypeId::of::<T>())
    }

    /// The registry attached to the request a server function or a server
    /// render is handling, for reading a resource inside the body the way
    /// Dioxus's `FullstackContext::extension` reads a request extension:
    ///
    /// ```rust,ignore
    /// #[server]
    /// async fn bump() -> Result<u64> {
    ///     let clicks = Resources::current()?.try_get::<Clicks>()?;
    ///     Ok(clicks.bump())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Fails with a `500` outside a server function or server render, where
    /// Dioxus sets no request context: during startup, in a task spawned from
    /// one (hand it the registry before spawning, like any request context),
    /// or in a plain axum handler or middleware, which take [`Res<T>`](struct@crate::Res)
    /// or `Extension<Resources>` instead. Also fails when the router serving
    /// the request has no registry attached.
    #[cfg(feature = "server")]
    #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
    pub fn current() -> Result<Self, HttpError> {
        let Some(context) = FullstackContext::current() else {
            return Err(HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "haze Resources were requested outside a server function or server render: during startup, in a task spawned from one, or in a plain axum handler or middleware, which takes Res<T> or Extension<Resources>",
            ));
        };
        context.extension::<Self>().ok_or_else(|| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "no haze Resources are attached to this router; start it with haze::serve or layer Extension(resources)",
            )
        })
    }

    /// Obtains every [`#[register]`](macro@crate::register)ed implementation of
    /// the trait `T`, building the [`Build`](crate::Build) types and cloning
    /// the others from these resources, sorted by `order`, then by type name,
    /// and inserts them as a [`Seq<T>`](crate::Seq).
    ///
    /// Every implementation to clone is checked before anything is built, so an
    /// implementation still waiting for a pack does not rebuild the ones before it
    /// in the next round. A `Seq<T>` that is already inserted is kept and nothing is
    /// built, so calling this again does nothing.
    ///
    /// [`serve`](fn@crate::serve) does this for every registered trait after the
    /// resource functions run, while the packs install. Call it directly only to:
    ///
    /// - fill a `Seq<T>` in setup, which works when every implementation is a value
    ///   setup inserted or a `Build` type that needs only such values, since a
    ///   registered pack exists only after startup installs it;
    /// - insert an empty `Seq<T>` for a trait nobody registered.
    ///
    /// `T` must be written exactly as in the registered `impl Trait for Type`:
    /// `collect::<dyn Trait>()` finds them, `collect::<dyn Trait + Send>()` is a
    /// different type and finds none.
    ///
    /// # Errors
    ///
    /// Fails when an implementation cannot be obtained, naming it and its
    /// error: the first implementation to clone that was never inserted,
    /// otherwise the first `Build` that fails.
    ///
    /// # Panics
    ///
    /// Panics if another `Resources` clone still shares this registry.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// resources.collect::<dyn Interceptor>()?;
    /// ```
    #[track_caller]
    pub fn collect<T: ?Sized + Send + Sync + 'static>(&mut self) -> Result<()> {
        if self.contains::<Seq<T>>() {
            return Ok(());
        }
        let context = |registration: &Registration| {
            format!("collecting {} as {}", registration.name(), type_name::<T>())
        };
        let registrations = Registration::of::<T>();
        for registration in &registrations {
            registration
                .check(self)
                .with_context(|| context(registration))?;
        }
        let mut collected = Vec::new();
        for registration in registrations {
            let built = registration
                .build(self)
                .with_context(|| context(registration))?;
            let Ok(boxed) = built.downcast::<Box<T>>() else {
                unreachable!(
                    "{} was not built as {}",
                    registration.name(),
                    type_name::<T>()
                );
            };
            collected.push(*boxed);
        }
        self.insert(Seq::from(collected));
        Ok(())
    }

    /// Runs every [`#[resource]`](macro@crate::resource) function whose type is
    /// not inserted yet and inserts what it returns.
    ///
    /// A function runs once every required `T` it takes is present, so functions may
    /// depend on each other in any order.
    ///
    /// - A function that takes `Option<T>` waits while another function will still
    ///   provide `T`.
    /// - A cycle of waiting functions that goes through at least one `Option<T>` is
    ///   broken only once it waits on nothing outside itself. Its first function by
    ///   name whose required types exist runs with `None` for the cycle's types.
    ///   Functions outside the cycle keep waiting and get their value.
    /// - Before anything runs, startup fails if some function can never get its
    ///   required types.
    /// - A type inserted by hand is kept and its function is skipped, so calling
    ///   this again does nothing.
    /// - A [`Later`] handed to a function stays empty until
    ///   [`finish`](Self::finish), unless its type already exists.
    ///
    /// [`serve`](fn@crate::serve) and [`start`](Self::start) call this after setup,
    /// then install the registered traits and packs, which this method does not.
    ///
    /// # Errors
    ///
    /// Fails when two functions provide the same type (even if that type was
    /// also inserted by hand), when a function returns an error, or when
    /// functions are left that cannot run, listing what each one is missing and
    /// each cycle of required parameters once.
    ///
    /// # Panics
    ///
    /// Panics if another `Resources` clone still shares this registry.
    pub async fn provide(&mut self) -> Result<()> {
        Provider::resolve(self).await
    }

    /// Returns a [`Later<T>`](Later): filled right away if `T` is already
    /// inserted, otherwise filled by [`finish`](Self::finish).
    ///
    /// # Panics
    ///
    /// Panics if called after startup finished for a `T` that was never
    /// inserted, since nothing could fill it anymore.
    #[must_use]
    #[track_caller]
    pub fn later<T: Clone + Send + Sync + 'static>(&self) -> Later<T> {
        if let Some(found) = self.get::<T>() {
            return Later::new(Arc::new(OnceLock::from(found)));
        }
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(fills) = pending.as_mut() else {
            panic!(
                "Later<{0}> requested after startup finished, but {0} was never inserted",
                type_name::<T>()
            );
        };
        let value = Arc::new(OnceLock::new());
        let slot = Arc::clone(&value);
        fills.push(Box::new(move |resources: &Self| {
            let found = resources.get::<T>().ok_or_else(|| {
                anyhow!(
                    "Later<{0}> needs {0}, which was never inserted",
                    type_name::<T>()
                )
            })?;
            let _ = slot.set(found);
            Ok(())
        }));
        Later::new(value)
    }

    /// Ends startup: fills every [`Later`] handed out by these resources, then
    /// forgets them. [`serve`](fn@crate::serve) calls this after setup, the
    /// resource functions, the registered traits and the packs. Calling it
    /// again does nothing.
    ///
    /// # Errors
    ///
    /// Fails when a `Later<T>` was handed out for a `T` that was never inserted,
    /// so the mistake stops startup instead of the first request that uses it.
    pub fn finish(&self) -> Result<()> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_default();
        let mut missing = Vec::new();
        for fill in pending {
            if let Err(error) = fill(self) {
                missing.push(error.to_string());
            }
        }
        if missing.is_empty() {
            return Ok(());
        }
        missing.sort();
        missing.dedup();
        bail!("{}", missing.join("; "))
    }

    /// Builds a registry the way [`serve`](fn@crate::serve) does on first start:
    /// runs `setup`, then [`provide`](Self::provide), then
    /// [`collect`](Self::collect) for every registered trait and the build of
    /// every [`#[derive(Pack)]`](derive@crate::Pack) struct, then
    /// [`finish`](Self::finish).
    ///
    /// Use it to test an app's startup without a server, in a custom launcher
    /// that attaches the result with `Extension(resources)`, or in a standalone
    /// app that makes the result the process default with
    /// [`install_default`](Self::install_default), as
    /// [`install`](fn@crate::install) does. Call it once: Dioxus runs
    /// the closure given to `dioxus::server::serve` again after every server
    /// hot-patch, so building inside it would reopen every resource.
    /// [`serve`](fn@crate::serve) runs it once and reuses the result.
    ///
    /// Registered traits and packs are installed after the resource functions, in
    /// rounds, ordered by the type each one inserts.
    ///
    /// - One that needs another trait's `Seq` or another pack waits for it.
    /// - When nothing more can be installed, startup fails, naming every item left
    ///   with its error.
    /// - An `Option` pack field of a `Seq` or of another pack is filled only if that
    ///   item was installed earlier.
    /// - Resource functions run before any of them. One that needs a `Seq` or a pack
    ///   takes it as [`Later<T>`](Later). As `T` it stops startup, and as `Option<T>`
    ///   it is always `None`.
    ///
    /// # Errors
    ///
    /// Fails with the first error from `setup`, a resource function, the
    /// installation of the registered traits and packs, or `finish`.
    ///
    /// # Panics
    ///
    /// Panics if `setup` keeps a clone of the registry, which would then still
    /// share it when the resource functions, registered traits and packs insert
    /// their values.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// #[tokio::test]
    /// async fn the_app_starts() {
    ///     let resources = Resources::start(async |resources| {
    ///         resources.insert(Clicks::default());
    ///         Ok(())
    ///     })
    ///     .await
    ///     .unwrap();
    ///     assert!(resources.contains::<Clicks>());
    /// }
    /// ```
    pub async fn start<S>(setup: S) -> Result<Self>
    where
        S: AsyncFnOnce(&mut Self) -> Result<()>,
    {
        let mut resources = Self::new();
        setup(&mut resources).await?;
        resources.provide().await?;
        Installer::install(&mut resources)?;
        resources.finish()?;
        Ok(resources)
    }

    /// Makes this registry the default for this process, which
    /// [`get_default`](Self::get_default) returns from then on.
    ///
    /// A standalone app has no request to attach the registry to, so it calls
    /// this once, after [`start`](Self::start), and code anywhere in the
    /// process reads its resources through `get_default`. It succeeds at most
    /// once per process: the default is never replaced, and it lives until the
    /// process exits, so its values are never dropped.
    ///
    /// # Errors
    ///
    /// Fails when a default is already installed, handing this registry back.
    ///
    /// # Examples
    ///
    /// ```
    /// use haze::Resources;
    ///
    /// let mut resources = Resources::new();
    /// resources.insert(String::from("data.redb"));
    /// resources.install_default().unwrap();
    ///
    /// let installed = Resources::get_default().unwrap();
    /// assert_eq!(installed.get::<String>().as_deref(), Some("data.redb"));
    /// assert!(Resources::new().install_default().is_err());
    /// ```
    pub fn install_default(self) -> Result<(), Self> {
        PROCESS_DEFAULT_RESOURCES.set(self)
    }

    /// The process default, built by `init` when none is installed yet and
    /// reused by every later call, so startup runs at most once per process.
    #[cfg(all(feature = "standalone", not(target_family = "wasm")))]
    pub(crate) fn get_or_install(init: impl FnOnce() -> Self) -> &'static Self {
        PROCESS_DEFAULT_RESOURCES.get_or_init(init)
    }

    /// The registry [`install_default`](Self::install_default) made the
    /// default for this process, or `None` if none is installed yet.
    #[must_use]
    pub fn get_default() -> Option<&'static Self> {
        PROCESS_DEFAULT_RESOURCES.get()
    }

    /// Whether a value was inserted under this [`TypeId`], for callers that only
    /// know types by id.
    pub(crate) fn contains_type_id(&self, type_id: TypeId) -> bool {
        self.slots.contains_key(&type_id)
    }
}

impl Debug for Resources {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let mut names = Vec::with_capacity(self.slots.len());
        for (name, _) in self.slots.values() {
            names.push(*name);
        }
        names.sort_unstable();
        formatter
            .debug_struct("Resources")
            .field("types", &names)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Display, sync::Arc};

    use super::Resources;

    #[test]
    fn a_missing_type_is_none() {
        assert!(Resources::new().get::<u64>().is_none());
    }

    #[test]
    fn get_returns_a_clone_of_the_inserted_value() {
        let mut resources = Resources::new();
        resources.insert(String::from("storage"));
        assert_eq!(resources.get::<String>().unwrap(), "storage");
    }

    #[test]
    fn an_inserted_arc_is_shared_not_copied() {
        let original = Arc::new(42_u64);
        let mut resources = Resources::new();
        resources.insert(original.clone());

        let retrieved = resources.get::<Arc<u64>>().unwrap();

        assert!(Arc::ptr_eq(&original, &retrieved));
    }

    #[test]
    fn lookup_uses_the_exact_inserted_type() {
        let mut resources = Resources::new();
        resources.insert(Arc::new(42_u64));
        assert!(resources.get::<u64>().is_none());
        assert!(resources.get::<Arc<u64>>().is_some());
    }

    #[test]
    fn a_trait_object_is_inserted_behind_an_arc() {
        let value: Arc<dyn Display + Send + Sync> = Arc::new(42_u64);
        let mut resources = Resources::new();
        resources.insert(value);
        let found = resources.get::<Arc<dyn Display + Send + Sync>>().unwrap();
        assert_eq!(found.to_string(), "42");
    }

    #[test]
    fn contains_reports_inserted_types() {
        let mut resources = Resources::new();
        resources.insert(1_u64);
        assert!(resources.contains::<u64>());
        assert!(!resources.contains::<u32>());
    }

    #[test]
    fn try_get_names_the_missing_type() {
        let mut resources = Resources::new();
        resources.insert(1_u64);
        assert_eq!(resources.try_get::<u64>().unwrap(), 1);
        let error = resources.try_get::<u32>().unwrap_err();
        assert_eq!(error.to_string(), "u32 was never inserted");
    }

    #[test]
    #[should_panic(expected = "u64 inserted twice")]
    fn inserting_a_type_twice_names_it() {
        let mut resources = Resources::new();
        resources.insert(1_u64);
        resources.insert(2_u64);
    }

    #[test]
    #[should_panic(
        expected = "u64 inserted while Resources is shared; insert everything during setup"
    )]
    fn inserting_while_shared_names_the_type() {
        let mut resources = Resources::new();
        let _shared = resources.clone();
        resources.insert(1_u64);
    }

    #[test]
    fn separate_registries_are_independent() {
        let mut first = Resources::new();
        let mut second = Resources::new();
        first.insert(1_u64);
        second.insert(2_u64);
        assert_eq!(first.get::<u64>(), Some(1));
        assert_eq!(second.get::<u64>(), Some(2));
    }

    #[test]
    fn debug_lists_types_without_values() {
        let mut resources = Resources::new();
        resources.insert(42_u64);
        resources.insert(String::from("secret-value"));
        let debug = format!("{resources:?}");
        assert!(debug.find("String").unwrap() < debug.find("u64").unwrap());
        assert!(!debug.contains("secret-value"));
        assert!(!debug.contains("42"));
    }
}
