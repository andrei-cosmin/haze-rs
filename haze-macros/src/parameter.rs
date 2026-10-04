//! The three resource forms a `#[haze::resource]` parameter and a
//! `#[derive(Pack)]` field share.

use syn::{Error, GenericArgument, PathArguments, Type};

/// One resource taken by a `#[haze::resource]` function or a `#[derive(Pack)]`
/// field, classified by its type the way summer's `InjectableType` classifies
/// a service field. One classifier serves both macros, the way serde's
/// `Container::from_ast` serves both of its derives, so the two accept exactly
/// the same forms.
pub enum Parameter<'a> {
    /// `T`: waits until `T` exists.
    Required(&'a Type),
    /// `Option<T>`: `None` when no `T` exists at the moment of the read.
    Optional(&'a Type),
    /// `Later<T>`: not waited for, filled by the end of startup.
    Later(&'a Type),
}

impl<'a> Parameter<'a> {
    /// Classifies `ty`.
    ///
    /// Returns a compile error starting with `subject`, such as
    /// `a #[haze::resource] parameter`, for:
    /// - `Res<T>`, alone or directly inside `Option` or `Later`;
    /// - `Option<Later<T>>` and `Later<Later<T>>`;
    /// - `impl Trait`;
    /// - a reference that is not shared and `'static`.
    pub fn of(ty: &'a Type, subject: &str) -> Result<Self, Error> {
        if let Some(inner) = Self::wrapped(ty, "Later") {
            if Self::wrapped(inner, "Later").is_some() {
                return Err(Self::rejected(
                    ty,
                    subject,
                    "takes `Later<T>`, not `Later<Later<T>>`",
                ));
            }
            return Ok(Self::Later(Self::bare(inner, ty, subject)?));
        }
        if let Some(inner) = Self::wrapped(ty, "Option") {
            if Self::wrapped(inner, "Later").is_some() {
                return Err(Self::rejected(
                    ty,
                    subject,
                    "takes `Later<T>`, not `Option<Later<T>>`",
                ));
            }
            return Ok(Self::Optional(Self::bare(inner, ty, subject)?));
        }
        Ok(Self::Required(Self::bare(ty, ty, subject)?))
    }

    /// Returns `inner` unless it is `Res<T>`, `impl Trait` or a reference that
    /// is not a shared `'static` one, like serde's `is_reference`; `whole` is
    /// the type the error points at.
    fn bare(inner: &'a Type, whole: &Type, subject: &str) -> Result<&'a Type, Error> {
        if Self::wrapped(inner, "Res").is_some() {
            return Err(Self::rejected(
                whole,
                subject,
                "takes `T`, `Option<T>` or `Later<T>`, not `Res<T>`; `Res` is for server functions, and a type of your own named `Res` needs a type alias here",
            ));
        }
        match Self::ungroup(inner) {
            Type::ImplTrait(_) => Err(Self::rejected(
                whole,
                subject,
                "names a concrete type, not `impl Trait`",
            )),
            Type::Reference(reference)
                if reference.mutability.is_some()
                    || reference
                        .lifetime
                        .as_ref()
                        .is_none_or(|lifetime| lifetime.ident != "static") =>
            {
                Err(Self::rejected(
                    whole,
                    subject,
                    "is taken by value; a reference resource is `&'static T`",
                ))
            }
            _ => Ok(inner),
        }
    }

    /// A compile error on `ty` reading "`subject` `rule`".
    fn rejected(ty: &Type, subject: &str, rule: &str) -> Error {
        Error::new_spanned(ty, format!("{subject} {rule}"))
    }

    /// Returns `T` when `ty` is written `Wrapper<T>`, matching the last path
    /// segment by name like summer's `is_component_type`, then taking its first
    /// generic argument like summer's `extract_generic_type`.
    pub fn wrapped(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
        let Type::Path(path) = Self::ungroup(ty) else {
            return None;
        };
        let segment = path.path.segments.last()?;
        if segment.ident != wrapper {
            return None;
        }
        let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
            return None;
        };
        let Some(GenericArgument::Type(inner)) = arguments.args.first() else {
            return None;
        };
        Some(inner)
    }

    /// Strips the invisible groups a type arrives in when it came through a
    /// `macro_rules!` `$ty` fragment, like serde's `ungroup`.
    pub fn ungroup(mut ty: &'a Type) -> &'a Type {
        while let Type::Group(group) = ty {
            ty = &group.elem;
        }
        ty
    }

    /// The resource type `T` inside the wrapper, or `T` itself.
    pub fn inner(&self) -> &'a Type {
        match self {
            Self::Required(inner) | Self::Optional(inner) | Self::Later(inner) => inner,
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::ToTokens;
    use syn::{Type, TypeGroup, parse_quote, token::Group};

    use super::Parameter;

    struct Classified;

    impl Classified {
        fn error(ty: &Type) -> String {
            let Err(error) = Parameter::of(ty, "a thing") else {
                panic!("the form was accepted");
            };
            error.to_string()
        }
    }

    #[test]
    fn a_type_from_a_macro_rules_fragment_is_unwrapped() {
        let grouped = Type::Group(TypeGroup {
            group_token: Group::default(),
            elem: Box::new(parse_quote!(Option<Motto>)),
        });
        let inner = Parameter::wrapped(&grouped, "Option").unwrap();
        assert_eq!(inner.to_token_stream().to_string(), "Motto");
        assert!(Parameter::wrapped(&grouped, "Later").is_none());
    }

    #[test]
    fn nested_groups_are_all_stripped() {
        let inner = Type::Group(TypeGroup {
            group_token: Group::default(),
            elem: Box::new(parse_quote!(impl Store)),
        });
        let outer = Type::Group(TypeGroup {
            group_token: Group::default(),
            elem: Box::new(inner),
        });
        assert!(matches!(Parameter::ungroup(&outer), Type::ImplTrait(_)));
    }

    #[test]
    fn the_three_forms_are_classified() {
        let required: Type = parse_quote!(Store);
        assert!(matches!(
            Parameter::of(&required, "a thing"),
            Ok(Parameter::Required(_))
        ));
        let optional: Type = parse_quote!(Option<Store>);
        assert!(matches!(
            Parameter::of(&optional, "a thing"),
            Ok(Parameter::Optional(_))
        ));
        let later: Type = parse_quote!(Later<Store>);
        assert!(matches!(
            Parameter::of(&later, "a thing"),
            Ok(Parameter::Later(_))
        ));
        let leaked: Type = parse_quote!(&'static str);
        assert!(matches!(
            Parameter::of(&leaked, "a thing"),
            Ok(Parameter::Required(_))
        ));
        let doubled: Type = parse_quote!(Option<Option<Store>>);
        let Ok(Parameter::Optional(inner)) = Parameter::of(&doubled, "a thing") else {
            panic!("Option<Option<T>> is an optional Option<T>");
        };
        assert_eq!(inner.to_token_stream().to_string(), "Option < Store >");
    }

    #[test]
    fn rejections_name_the_subject_and_the_rule() {
        let res = "a thing takes `T`, `Option<T>` or `Later<T>`, not `Res<T>`; `Res` is for server functions, and a type of your own named `Res` needs a type alias here";
        assert_eq!(Classified::error(&parse_quote!(Res<Store>)), res);
        assert_eq!(Classified::error(&parse_quote!(Option<Res<Store>>)), res);
        assert_eq!(Classified::error(&parse_quote!(Later<Res<Store>>)), res);
        assert_eq!(
            Classified::error(&parse_quote!(Option<Later<Store>>)),
            "a thing takes `Later<T>`, not `Option<Later<T>>`"
        );
        assert_eq!(
            Classified::error(&parse_quote!(Later<Later<Store>>)),
            "a thing takes `Later<T>`, not `Later<Later<T>>`"
        );
        assert_eq!(
            Classified::error(&parse_quote!(&Store)),
            "a thing is taken by value; a reference resource is `&'static T`"
        );
        assert_eq!(
            Classified::error(&parse_quote!(&'static mut Store)),
            "a thing is taken by value; a reference resource is `&'static T`"
        );
        assert_eq!(
            Classified::error(&parse_quote!(impl Store)),
            "a thing names a concrete type, not `impl Trait`"
        );
    }
}
