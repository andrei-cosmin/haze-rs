//! The record `#[haze::register]` submits for `Resources::collect`.

use std::{
    any::{Any, TypeId},
    fmt::{self, Debug, Formatter},
};

use anyhow::Result;

use crate::Resources;

/// Fails when the registered type is cloned from the registry but was never
/// inserted; passes for a type that is built.
type Check = fn(&Resources) -> Result<()>;

/// Obtains one registered type, built or cloned, and pushes it as
/// `Box<dyn Trait>` onto the `Vec<Box<dyn Trait>>` passed as `dyn Any`.
type Builder = fn(&Resources, &mut dyn Any) -> Result<()>;

/// One implementation recorded by `#[haze::register]`: which trait it
/// implements, its order, its name, and how to check for and obtain it as
/// `Box<dyn Trait>`.
#[doc(hidden)]
pub struct Registration {
    /// Returns the [`TypeId`] of the `dyn Trait` this implementation is collected as.
    trait_id: fn() -> TypeId,
    /// Its place in the collected sequence, lowest first.
    order: i32,
    /// Returns the implementing type's full path, for error messages and tie-breaks.
    name: fn() -> &'static str,
    /// Fails when the implementing type is cloned from the registry and was
    /// never inserted; passes for a `Build` type.
    check: Check,
    /// Obtains the implementing type, built or cloned, and boxes it as
    /// `Box<dyn Trait>`.
    build: Builder,
}

inventory::collect!(Registration);

impl Registration {
    /// Describes one registered implementation.
    #[must_use]
    pub const fn new(
        trait_id: fn() -> TypeId,
        order: i32,
        name: fn() -> &'static str,
        check: Check,
        build: Builder,
    ) -> Self {
        Self {
            trait_id,
            order,
            name,
            check,
            build,
        }
    }

    /// Every registration for the trait object type `T`, sorted by order, then name.
    pub(crate) fn of<T: ?Sized + 'static>() -> Vec<&'static Self> {
        let mut matching: Vec<&'static Self> = inventory::iter::<Self>
            .into_iter()
            .filter(|registration| (registration.trait_id)() == TypeId::of::<T>())
            .collect();
        matching.sort_by_key(|registration| (registration.order, registration.name()));
        matching
    }

    /// The registered type's full path, for error messages.
    pub(crate) fn name(&self) -> &'static str {
        (self.name)()
    }

    /// Fails when the implementation is cloned from the registry and its type
    /// was never inserted.
    pub(crate) fn check(&self, resources: &Resources) -> Result<()> {
        (self.check)(resources)
    }

    /// Obtains the implementation, built or cloned, and pushes it as
    /// `Box<dyn Trait>` onto `collected`, a `Vec<Box<dyn Trait>>`.
    pub(crate) fn build(&self, resources: &Resources, collected: &mut dyn Any) -> Result<()> {
        (self.build)(resources, collected)
    }
}

impl Debug for Registration {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Registration")
            .field("name", &self.name())
            .field("order", &self.order)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::any::{Any, TypeId};

    use anyhow::Result;

    use super::Registration;
    use crate::Resources;

    struct Sample;

    impl Sample {
        fn trait_id() -> TypeId {
            TypeId::of::<dyn Any>()
        }

        fn name() -> &'static str {
            "tests::Sample"
        }

        #[allow(clippy::unnecessary_wraps)]
        fn check(_resources: &Resources) -> Result<()> {
            Ok(())
        }

        #[allow(clippy::unnecessary_wraps)]
        fn build(_resources: &Resources, _collected: &mut dyn Any) -> Result<()> {
            Ok(())
        }
    }

    const SAMPLE: Registration = Registration::new(
        Sample::trait_id,
        7,
        Sample::name,
        Sample::check,
        Sample::build,
    );

    #[test]
    fn debug_names_the_type_and_order() {
        assert_eq!(
            format!("{SAMPLE:?}"),
            "Registration { name: \"tests::Sample\", order: 7, .. }"
        );
    }
}
