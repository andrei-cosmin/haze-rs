//! `Need`: one resource type named for error messages and keyed by `TypeId`.

use std::{
    any::{TypeId, type_name},
    fmt::{self, Debug, Formatter},
};

/// One resource type, named for error messages and keyed by its [`TypeId`].
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct Need {
    /// Returns the type's full path.
    name: fn() -> &'static str,
    /// Returns the type's [`TypeId`].
    id: fn() -> TypeId,
}

impl Need {
    /// Describes the type `T`.
    #[must_use]
    pub const fn of<T: 'static>() -> Self {
        Self {
            name: type_name::<T>,
            id: TypeId::of::<T>,
        }
    }

    /// The type's full path.
    pub(crate) fn name(&self) -> &'static str {
        (self.name)()
    }

    /// The type's [`TypeId`].
    pub(crate) fn id(&self) -> TypeId {
        (self.id)()
    }
}

impl Debug for Need {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::Need;

    #[test]
    fn debug_is_the_type_name() {
        assert_eq!(format!("{:?}", Need::of::<u64>()), "u64");
    }
}
