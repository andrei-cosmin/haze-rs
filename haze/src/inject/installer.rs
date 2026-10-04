//! The record that installs one item after the resource functions: the
//! `Seq<dyn Trait>` of a `#[haze::register]`ed trait, or a pack.

use std::fmt::{self, Debug, Formatter};

use anyhow::{Result, bail};

use crate::{Pack, Resources, inject::need::Need};

/// Builds one item from the registry and inserts it.
type Install = fn(&mut Resources) -> Result<()>;

/// One item built after the resource functions: what it inserts and how, the
/// way summer's `ServiceRegistrar` installs one service.
#[doc(hidden)]
pub struct Installer {
    /// The type the item inserts: it sets the startup order, keeps one record
    /// per type, and is shown by `Debug`.
    inserts: Need,
    /// Builds the item from the registry and inserts it, or does nothing when
    /// the type is already present.
    install: Install,
}

inventory::collect!(Installer);

impl Installer {
    /// Describes one item to install.
    #[must_use]
    pub const fn new(inserts: Need, install: Install) -> Self {
        Self { inserts, install }
    }

    /// Describes the pack `T`, built with [`Pack::build`] and inserted under its
    /// own type, the way summer's `Service` derive submits one registrar per
    /// service.
    #[must_use]
    pub const fn of<T: Pack>() -> Self {
        Self::new(Need::of::<T>(), Self::build_into::<T>)
    }

    /// Builds `T` and inserts it, keeping a `T` that is already inserted.
    fn build_into<T: Pack>(resources: &mut Resources) -> Result<()> {
        if resources.contains::<T>() {
            return Ok(());
        }
        let pack = T::build(resources)?;
        resources.insert(pack);
        Ok(())
    }

    /// Installs every registered item once, sorted by the type it inserts so
    /// startup runs in the same order on every build; every `#[haze::register]`
    /// on one trait submits the same collector, so one record per type is kept,
    /// the way bevy's type registry skips a `TypeId` it already holds.
    pub(crate) fn install(resources: &mut Resources) -> Result<()> {
        let mut installers = Vec::new();
        for installer in inventory::iter::<Self> {
            installers.push(installer);
        }
        installers.sort_by_key(|installer| (installer.inserts.name(), installer.inserts.id()));
        installers.dedup_by_key(|installer| installer.inserts.id());
        Self::install_all(resources, installers)
    }

    /// Installs the given items, so tests can pass their own instead of the
    /// registered ones. An item that fails waits for the next round, since it
    /// may need one installed later; once a round installs nothing, every item
    /// left is named with its error.
    fn install_all(resources: &mut Resources, installers: Vec<&Self>) -> Result<()> {
        let mut pending = installers;
        while !pending.is_empty() {
            let mut progress = false;
            let mut next_round = Vec::new();
            let mut failures = Vec::new();
            for installer in pending {
                match (installer.install)(resources) {
                    Ok(()) => progress = true,
                    Err(error) => {
                        failures.push(format!("{error:#}"));
                        next_round.push(installer);
                    }
                }
            }
            if !progress {
                failures.sort();
                bail!("{}", failures.join("; "));
            }
            pending = next_round;
        }
        Ok(())
    }
}

impl Debug for Installer {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Installer")
            .field("inserts", &self.inserts)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::Installer;
    use crate::{Pack, Resources, inject::need::Need};

    #[derive(Clone)]
    struct Inner(u8);

    impl Pack for Inner {
        fn build(resources: &Resources) -> Result<Self> {
            Ok(Self(resources.try_get::<u8>()?))
        }
    }

    #[derive(Clone)]
    struct Outer(u8);

    impl Pack for Outer {
        fn build(resources: &Resources) -> Result<Self> {
            Ok(Self(resources.try_get::<Inner>()?.0 + 1))
        }
    }

    #[derive(Clone)]
    struct Wide;

    impl Pack for Wide {
        fn build(resources: &Resources) -> Result<Self> {
            resources.try_get::<Inner>()?;
            resources.try_get::<u16>()?;
            Ok(Self)
        }
    }

    const INNER: Installer = Installer::of::<Inner>();
    const OUTER: Installer = Installer::of::<Outer>();
    const WIDE: Installer = Installer::of::<Wide>();

    struct Fixtures;

    impl Fixtures {
        #[allow(clippy::unnecessary_wraps)]
        fn number(resources: &mut Resources) -> Result<()> {
            if resources.contains::<u64>() {
                return Ok(());
            }
            resources.insert(7_u64);
            Ok(())
        }

        fn text(resources: &mut Resources) -> Result<()> {
            if resources.contains::<String>() {
                return Ok(());
            }
            let number = resources.try_get::<u64>()?;
            resources.insert(format!("number {number}"));
            Ok(())
        }

        fn length(resources: &mut Resources) -> Result<()> {
            if resources.contains::<usize>() {
                return Ok(());
            }
            let text = resources.try_get::<String>()?;
            resources.insert(text.len());
            Ok(())
        }

        fn starved(resources: &mut Resources) -> Result<()> {
            resources.try_get::<u8>()?;
            Ok(())
        }
    }

    const NUMBER: Installer = Installer::new(Need::of::<u64>(), Fixtures::number);
    const TEXT: Installer = Installer::new(Need::of::<String>(), Fixtures::text);
    const LENGTH: Installer = Installer::new(Need::of::<usize>(), Fixtures::length);
    const STARVED: Installer = Installer::new(Need::of::<i8>(), Fixtures::starved);

    #[test]
    fn items_are_retried_until_what_they_need_is_installed() {
        let mut resources = Resources::new();
        Installer::install_all(&mut resources, vec![&LENGTH, &TEXT, &NUMBER]).unwrap();
        assert_eq!(resources.get::<usize>(), Some("number 7".len()));
    }

    #[test]
    fn an_item_already_inserted_is_kept() {
        let mut resources = Resources::new();
        resources.insert(5_u64);
        Installer::install_all(&mut resources, vec![&NUMBER, &TEXT]).unwrap();
        assert_eq!(resources.get::<String>().unwrap(), "number 5");
    }

    #[test]
    fn when_nothing_more_can_be_installed_every_item_left_is_named_in_order() {
        let error =
            Installer::install_all(&mut Resources::new(), vec![&STARVED, &TEXT]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "u64 was never inserted; u8 was never inserted"
        );
    }

    #[test]
    fn a_pack_listed_before_the_pack_it_holds_is_built_in_the_next_round() {
        let mut resources = Resources::new();
        resources.insert(1_u8);
        Installer::install_all(&mut resources, vec![&OUTER, &INNER]).unwrap();
        assert_eq!(resources.get::<Outer>().unwrap().0, 2);
    }

    #[test]
    fn a_pack_already_inserted_is_kept_and_not_built() {
        let mut resources = Resources::new();
        resources.insert(Inner(40));
        Installer::install_all(&mut resources, vec![&OUTER, &INNER]).unwrap();
        assert_eq!(resources.get::<Outer>().unwrap().0, 41);
    }

    #[test]
    fn only_the_errors_of_the_round_that_installed_nothing_are_named() {
        let mut resources = Resources::new();
        resources.insert(1_u8);
        let error = Installer::install_all(&mut resources, vec![&WIDE, &INNER]).unwrap_err();
        assert_eq!(error.to_string(), "u16 was never inserted");
    }

    #[test]
    fn debug_names_what_it_inserts() {
        assert_eq!(format!("{NUMBER:?}"), "Installer { inserts: u64, .. }");
    }

    #[derive(Clone)]
    struct Submitted;

    impl Submitted {
        #[allow(clippy::unnecessary_wraps)]
        fn install(resources: &mut Resources) -> Result<()> {
            resources.insert(Self);
            Ok(())
        }
    }

    inventory::submit! { Installer::new(Need::of::<Submitted>(), Submitted::install) }
    inventory::submit! { Installer::new(Need::of::<Submitted>(), Submitted::install) }

    #[test]
    fn one_installer_per_type_is_run() {
        let mut resources = Resources::new();
        Installer::install(&mut resources).unwrap();
        assert!(resources.contains::<Submitted>());
    }
}
