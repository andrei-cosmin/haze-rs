//! The four ways a server function handle names its resource.

use syn::Type;

use crate::parameter::Parameter;

/// How a handle of `#[haze::server]` or a route attribute is written, with
/// the resource type `T` inside it, classified by the last segment of each
/// wrapper's path like [`Parameter`].
pub enum Form<'a> {
    /// `T`: the resource itself; the call fails when `T` is missing.
    Required(&'a Type),
    /// `Option<T>`: `None` when `T` is missing.
    Optional(&'a Type),
    /// `Res<T>`: the resource in Dioxus's extractor form.
    Res(&'a Type),
    /// `Option<Res<T>>`: the optional extractor form.
    OptionalRes(&'a Type),
}

impl<'a> Form<'a> {
    /// Classifies `declared`, the outer `Option` first, then a `Res` inside or
    /// instead of it.
    pub fn of(declared: &'a Type) -> Self {
        if let Some(inner) = Parameter::wrapped(declared, "Option") {
            if let Some(resource) = Parameter::wrapped(inner, "Res") {
                return Self::OptionalRes(resource);
            }
            return Self::Optional(inner);
        }
        if let Some(resource) = Parameter::wrapped(declared, "Res") {
            return Self::Res(resource);
        }
        Self::Required(declared)
    }

    /// The resource type `T` inside the wrappers, or `T` itself.
    pub fn inner(&self) -> &'a Type {
        match self {
            Self::Required(inner)
            | Self::Optional(inner)
            | Self::Res(inner)
            | Self::OptionalRes(inner) => inner,
        }
    }
}
