//! The record `#[haze::collect(N)]` submits for `Resources::collect`.

use std::{
    any::TypeId,
    fmt::{self, Debug, Formatter},
};

use anyhow::{Result, bail};

use crate::inject::need::Need;

/// The exact number of implementations a `#[haze::collect(N)]` trait requires.
#[doc(hidden)]
pub struct Cardinality {
    /// The `dyn Trait` the count is for.
    counts: Need,
    /// The number of registered implementations the trait requires.
    count: usize,
}

inventory::collect!(Cardinality);

impl Cardinality {
    /// Describes the count one trait requires.
    #[must_use]
    pub const fn new(counts: Need, count: usize) -> Self {
        Self { counts, count }
    }

    /// Fails when a count is declared for the trait object type `T` and
    /// `found` registrations differ from it.
    pub(crate) fn check<T: ?Sized + 'static>(found: usize) -> Result<()> {
        for cardinality in inventory::iter::<Self> {
            if cardinality.counts.id() == TypeId::of::<T>() {
                cardinality.matches(found)?;
            }
        }
        Ok(())
    }

    /// Fails, naming the trait and both counts, when `found` is not the count.
    fn matches(&self, found: usize) -> Result<()> {
        if found != self.count {
            bail!(
                "{} is declared with #[haze::collect({})], so it needs exactly {} registered {}, but {} {} registered",
                self.counts.name(),
                self.count,
                self.count,
                Self::implementations(self.count),
                found,
                if found == 1 { "is" } else { "are" },
            );
        }
        Ok(())
    }

    /// The noun for `count` implementations.
    const fn implementations(count: usize) -> &'static str {
        if count == 1 {
            "implementation"
        } else {
            "implementations"
        }
    }
}

impl Debug for Cardinality {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Cardinality")
            .field("name", &self.counts.name())
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::Cardinality;
    use crate::inject::need::Need;

    trait Port {}

    const ONE: Cardinality = Cardinality::new(Need::of::<dyn Port>(), 1);
    const TWO: Cardinality = Cardinality::new(Need::of::<dyn Port>(), 2);

    #[test]
    fn the_declared_count_passes() {
        assert!(ONE.matches(1).is_ok());
        assert!(TWO.matches(2).is_ok());
    }

    #[test]
    fn another_count_names_the_trait_and_both_counts() {
        assert_eq!(
            ONE.matches(0).unwrap_err().to_string(),
            "dyn haze::inject::cardinality::tests::Port is declared with #[haze::collect(1)], so it needs exactly 1 registered implementation, but 0 are registered"
        );
        assert_eq!(
            TWO.matches(1).unwrap_err().to_string(),
            "dyn haze::inject::cardinality::tests::Port is declared with #[haze::collect(2)], so it needs exactly 2 registered implementations, but 1 is registered"
        );
    }

    #[test]
    fn debug_names_the_trait_and_count() {
        assert_eq!(
            format!("{ONE:?}"),
            "Cardinality { name: \"dyn haze::inject::cardinality::tests::Port\", count: 1, .. }"
        );
    }
}
