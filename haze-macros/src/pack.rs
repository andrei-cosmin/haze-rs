//! The `#[derive(Pack)]` expansion.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, spanned::Spanned};

use crate::field::Field;

/// Expands `#[derive(Pack)]` into `Pack` and an inventory `Installer` for a
/// struct with named fields, the way summer's `Service` derive generates
/// `Service::build` and submits a `ServiceRegistrar`.
pub struct Pack;

impl Pack {
    /// Generates the `Pack` impl and the `Installer`; or a compile error for a
    /// struct without named fields (an enum, union, tuple or unit struct), a
    /// generic struct, a field that cannot be fetched, or an unknown, malformed
    /// or repeated `#[pack]` attribute. Fields are bound in declaration order
    /// except `func` fields, which come last so they run once, after every
    /// other field was fetched, like summer's `FuncCall` sort.
    pub fn expand(input: &DeriveInput) -> Result<TokenStream, Error> {
        if !input.generics.params.is_empty() {
            return Err(Error::new_spanned(
                &input.generics,
                "a #[derive(Pack)] struct is not generic",
            ));
        }
        let Data::Struct(data) = &input.data else {
            return Err(Error::new(
                input.ident.span(),
                "a #[derive(Pack)] struct has named fields",
            ));
        };
        let Fields::Named(fields) = &data.fields else {
            return Err(Error::new(
                data.fields.span(),
                "a #[derive(Pack)] struct has named fields",
            ));
        };
        let name = &input.ident;
        let mut names = Vec::new();
        let mut kinds = Vec::new();
        for field in &fields.named {
            let field_name = field.ident.as_ref().ok_or_else(|| {
                Error::new(field.span(), "a #[derive(Pack)] struct has named fields")
            })?;
            kinds.push((Field::of(field)?, field_name));
            names.push(field_name);
        }
        kinds.sort_by_key(|(kind, _)| kind.is_func());
        let mut bindings = Vec::new();
        for (kind, field_name) in &kinds {
            bindings.push(kind.binding(field_name));
        }
        Ok(quote! {
            const _: () = {
                #[automatically_derived]
                impl ::haze::Pack for #name {
                    fn build(
                        __haze_resources: &::haze::Resources,
                    ) -> ::haze::__private::anyhow::Result<Self> {
                        #(#bindings)*
                        ::core::result::Result::Ok(Self { #(#names),* })
                    }
                }

                ::haze::__private::inventory::submit! {
                    ::haze::__private::Installer::of::<#name>()
                }
            };
        })
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;
    use syn::{DeriveInput, parse2};

    use super::Pack;

    struct Expansion;

    impl Expansion {
        fn of(input: TokenStream) -> Result<String, String> {
            let input = parse2::<DeriveInput>(input).unwrap();
            Pack::expand(&input)
                .map(|tokens| tokens.to_string())
                .map_err(|error| error.to_string())
        }
    }

    #[test]
    fn accepts_a_struct_of_resources() {
        let expanded = Expansion::of(quote! {
            struct Board { scores: Scores, motto: Option<Motto>, keeper: Later<Keeper> }
        })
        .unwrap();
        assert!(expanded.contains(":: haze :: Resources :: get :: < Scores > (__haze_resources)"));
        assert!(expanded.contains(":: haze :: Resources :: get :: < Motto > (__haze_resources)"));
        assert!(
            expanded.contains(":: haze :: Resources :: later :: < Keeper > (__haze_resources)")
        );
    }

    #[test]
    fn a_func_field_is_bound_after_every_other_field() {
        let expanded = Expansion::of(quote! {
            struct Board {
                #[pack(func = Self::label(&name))]
                label: String,
                name: String,
            }
        })
        .unwrap();
        let name = expanded.find("let name =").unwrap();
        let label = expanded.find("let label = Self :: label (& name)").unwrap();
        assert!(name < label);
    }

    #[test]
    fn only_the_pack_impl_and_its_installer_are_generated() {
        let expanded = Expansion::of(quote! { struct Board { scores: Scores } }).unwrap();
        assert_eq!(expanded.matches("impl ").count(), 1);
        assert!(expanded.contains("impl :: haze :: Pack for Board"));
        assert!(expanded.contains(":: haze :: __private :: Installer :: of :: < Board > ()"));
        assert!(!expanded.contains("FromRequestParts"));
        assert!(!expanded.contains("HttpError"));
    }

    #[test]
    fn a_field_may_be_named_resources() {
        let expanded = Expansion::of(quote! { struct Board { resources: Store } }).unwrap();
        assert!(expanded.contains(
            "let resources = :: core :: option :: Option :: ok_or_else (:: haze :: Resources :: get :: < Store > (__haze_resources)"
        ));
    }

    #[test]
    fn rejects_an_enum() {
        assert_eq!(
            Expansion::of(quote! { enum Board { Empty } }).unwrap_err(),
            "a #[derive(Pack)] struct has named fields"
        );
    }

    #[test]
    fn rejects_a_tuple_struct() {
        assert_eq!(
            Expansion::of(quote! { struct Board(Scores); }).unwrap_err(),
            "a #[derive(Pack)] struct has named fields"
        );
    }

    #[test]
    fn rejects_a_generic_struct() {
        assert_eq!(
            Expansion::of(quote! { struct Board<T> { scores: T } }).unwrap_err(),
            "a #[derive(Pack)] struct is not generic"
        );
    }

    #[test]
    fn rejects_a_res_field() {
        assert_eq!(
            Expansion::of(quote! { struct Board { motto: Res<Motto> } }).unwrap_err(),
            "a #[derive(Pack)] field takes `T`, `Option<T>` or `Later<T>`, not `Res<T>`; `Res` is for server functions, and a type of your own named `Res` needs a type alias here"
        );
    }

    #[test]
    fn rejects_an_optional_res_field() {
        assert_eq!(
            Expansion::of(quote! { struct Board { motto: Option<Res<Motto>> } }).unwrap_err(),
            "a #[derive(Pack)] field takes `T`, `Option<T>` or `Later<T>`, not `Res<T>`; `Res` is for server functions, and a type of your own named `Res` needs a type alias here"
        );
    }

    #[test]
    fn rejects_a_later_of_res_field() {
        assert_eq!(
            Expansion::of(quote! { struct Board { motto: Later<Res<Motto>> } }).unwrap_err(),
            "a #[derive(Pack)] field takes `T`, `Option<T>` or `Later<T>`, not `Res<T>`; `Res` is for server functions, and a type of your own named `Res` needs a type alias here"
        );
    }

    #[test]
    fn rejects_an_optional_later_field() {
        assert_eq!(
            Expansion::of(quote! { struct Board { keeper: Option<Later<Keeper>> } }).unwrap_err(),
            "a #[derive(Pack)] field takes `Later<T>`, not `Option<Later<T>>`"
        );
    }

    #[test]
    fn rejects_an_unknown_pack_argument() {
        assert_eq!(
            Expansion::of(quote! { struct Board { #[pack(default)] hits: u64 } }).unwrap_err(),
            "expected `#[pack(func = call(..))]`"
        );
    }

    #[test]
    fn rejects_a_repeated_pack_attribute() {
        assert_eq!(
            Expansion::of(quote! {
                struct Board {
                    #[pack(func = zero())]
                    #[pack(func = one())]
                    hits: u64,
                }
            })
            .unwrap_err(),
            "duplicate #[pack] attribute"
        );
    }
}
