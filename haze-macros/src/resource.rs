//! The `#[haze::resource]` expansion.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, ItemFn};

use crate::{parameter::Parameter, signature::Signature};

/// Expands `#[haze::resource]` on a function into an inventory `Provider`.
pub struct Resource;

impl Resource {
    /// Keeps the function, allowing clippy's `needless_pass_by_value` on it
    /// because the registry hands every parameter out as an owned clone, the
    /// way Dioxus's `#[component]` allows `non_snake_case` for its convention,
    /// and submits a `Provider` that fetches its parameters, calls it and
    /// inserts what it returns, or returns a compile error for an attribute
    /// argument, a generic function, a method receiver, a missing return type
    /// or one that is `impl Trait`, `Option<T>`, `Res<T>` or `Later<T>` (directly
    /// or inside `Result`), a parameter that is not `T`, `Option<T>` or
    /// `Later<T>` by value or as `&'static T` (so not `Res<T>`, `impl Trait`,
    /// `Option<Later<T>>` or `Later<Later<T>>`), or one that takes the type it
    /// provides.
    pub fn expand(attribute: TokenStream, function: &ItemFn) -> Result<TokenStream, Error> {
        if !attribute.is_empty() {
            return Err(Error::new_spanned(
                attribute,
                "a #[haze::resource] attribute takes no arguments",
            ));
        }
        let signature = Signature::parse(function)?;
        let name = signature.name;
        let provided = signature.provided;
        let mut arguments = Vec::new();
        let mut needs = Vec::new();
        let mut optional = Vec::new();
        for parameter in &signature.parameters {
            arguments.push(Self::argument(parameter));
            match parameter {
                Parameter::Required(inner) => needs.push(*inner),
                Parameter::Optional(inner) => optional.push(*inner),
                Parameter::Later(_) => {}
            }
        }
        let awaited = if signature.asynchronous {
            quote! { .await }
        } else {
            quote! {}
        };
        let checked = if signature.fallible {
            quote! {
                .map_err(|error| {
                    use ::haze::__private::error_kind::*;
                    ::haze::__private::anyhow::anyhow!((&error).haze_kind().prepare(error))
                })?
            }
        } else {
            quote! {}
        };
        Ok(quote! {
            #[allow(clippy::needless_pass_by_value)]
            #function

            const _: () = {
                fn __haze_provide(
                    __haze_resources: &mut ::haze::Resources,
                ) -> ::core::pin::Pin<
                    ::std::boxed::Box<
                        dyn ::core::future::Future<
                            Output = ::haze::__private::anyhow::Result<()>,
                        > + '_,
                    >,
                > {
                    ::std::boxed::Box::pin(async move {
                        let __haze_value = #name(#(#arguments),*) #awaited #checked;
                        ::haze::Resources::insert::<#provided>(__haze_resources, __haze_value);
                        ::core::result::Result::Ok(())
                    })
                }

                ::haze::__private::inventory::submit! {
                    ::haze::__private::Provider::new(
                        ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#name)),
                        ::haze::__private::Need::of::<#provided>(),
                        &[#(::haze::__private::Need::of::<#needs>()),*],
                        &[#(::haze::__private::Need::of::<#optional>()),*],
                        __haze_provide,
                    )
                }
            };
        })
    }

    /// The expression that produces one argument from the registry inside the
    /// generated provider, written as path calls like serde's generated code so
    /// a user trait in scope with a `get`, `try_get` or `later` method cannot
    /// take them over.
    fn argument(parameter: &Parameter<'_>) -> TokenStream {
        match parameter {
            Parameter::Required(inner) => quote! {
                ::haze::Resources::try_get::<#inner>(__haze_resources)?
            },
            Parameter::Optional(inner) => quote! {
                ::haze::Resources::get::<#inner>(__haze_resources)
            },
            Parameter::Later(inner) => quote! {
                ::haze::Resources::later::<#inner>(__haze_resources)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;
    use syn::{ItemFn, ReturnType, Type, TypeGroup, parse_quote, parse2, token::Group};

    use super::Resource;

    struct Expansion;

    impl Expansion {
        fn of(function: TokenStream) -> Result<(), String> {
            let function = parse2::<ItemFn>(function).unwrap();
            Resource::expand(TokenStream::new(), &function)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }

    #[test]
    fn accepts_a_function_of_resources() {
        assert!(
            Expansion::of(quote! {
                fn open(store: Store, cache: Option<Cache>, db: Later<Db>) -> Tunnel { todo!() }
            })
            .is_ok()
        );
    }

    #[test]
    fn rejects_the_server_function_forms() {
        for function in [
            quote! { fn open(store: Res<Store>) -> Tunnel { todo!() } },
            quote! { fn open(store: Option<Res<Store>>) -> Tunnel { todo!() } },
            quote! { fn open(store: Later<Res<Store>>) -> Tunnel { todo!() } },
        ] {
            assert_eq!(
                Expansion::of(function).unwrap_err(),
                "a #[haze::resource] parameter takes `T`, `Option<T>` or `Later<T>`, not `Res<T>`; `Res` is for server functions, and a type of your own named `Res` needs a type alias here"
            );
        }
    }

    #[test]
    fn rejects_a_reference_parameter_unless_it_is_static() {
        assert_eq!(
            Expansion::of(quote! { fn open(store: &Store) -> Tunnel { todo!() } }).unwrap_err(),
            "a #[haze::resource] parameter is taken by value; a reference resource is `&'static T`"
        );
        assert!(
            Expansion::of(quote! { fn open(name: &'static str) -> Tunnel { todo!() } }).is_ok()
        );
    }

    #[test]
    fn rejects_an_impl_trait_parameter() {
        assert_eq!(
            Expansion::of(quote! { fn open(store: impl Store) -> Tunnel { todo!() } }).unwrap_err(),
            "a #[haze::resource] parameter names a concrete type, not `impl Trait`"
        );
    }

    #[test]
    fn rejects_a_wrapped_return_type() {
        assert_eq!(
            Expansion::of(quote! { fn open() -> Res<Store> { todo!() } }).unwrap_err(),
            "a #[haze::resource] function returns `T`, not `Res<T>`; `Res` is for server functions"
        );
        assert_eq!(
            Expansion::of(quote! { fn open() -> Result<Later<Store>, Error> { todo!() } })
                .unwrap_err(),
            "a #[haze::resource] function returns `T`, not `Later<T>`; a function that needs a cycle takes `Later<T>`"
        );
    }

    #[test]
    fn rejects_an_option_return_type() {
        for function in [
            quote! { fn open() -> Option<Store> { todo!() } },
            quote! { fn open() -> Result<Option<Store>> { todo!() } },
        ] {
            assert_eq!(
                Expansion::of(function).unwrap_err(),
                "a #[haze::resource] function returns `T`, not `Option<T>`; a function that can do without it takes `Option<T>`"
            );
        }
    }

    #[test]
    fn accepts_an_async_function_returning_a_result() {
        assert!(
            Expansion::of(quote! { async fn open() -> Result<Store, Error> { todo!() } }).is_ok()
        );
    }

    #[test]
    fn rejects_a_method() {
        assert_eq!(
            Expansion::of(quote! { fn open(&self) -> Store { todo!() } }).unwrap_err(),
            "a #[haze::resource] function is a free function, not a method"
        );
    }

    #[test]
    fn rejects_nested_wrappers() {
        assert_eq!(
            Expansion::of(quote! { fn open(store: Later<Later<Store>>) -> Tunnel { todo!() } })
                .unwrap_err(),
            "a #[haze::resource] parameter takes `Later<T>`, not `Later<Later<T>>`"
        );
        assert_eq!(
            Expansion::of(quote! { fn open(store: Option<Later<Store>>) -> Tunnel { todo!() } })
                .unwrap_err(),
            "a #[haze::resource] parameter takes `Later<T>`, not `Option<Later<T>>`"
        );
    }

    #[test]
    fn rejects_a_function_without_a_return_value() {
        assert_eq!(
            Expansion::of(quote! { fn open(store: Store) { } }).unwrap_err(),
            "a #[haze::resource] function returns the resource it provides"
        );
    }

    #[test]
    fn rejects_an_impl_trait_return_type() {
        assert_eq!(
            Expansion::of(quote! { fn open() -> impl Store { todo!() } }).unwrap_err(),
            "a #[haze::resource] function returns a concrete type, not `impl Trait`"
        );
    }

    #[test]
    fn rejects_an_impl_trait_return_type_from_a_macro_rules_fragment() {
        let mut function =
            parse2::<ItemFn>(quote! { fn open() -> Placeholder { todo!() } }).unwrap();
        let ReturnType::Type(_, returned) = &mut function.sig.output else {
            unreachable!("the function returns a type");
        };
        **returned = Type::Group(TypeGroup {
            group_token: Group::default(),
            elem: Box::new(parse_quote!(impl Store)),
        });
        assert_eq!(
            Resource::expand(TokenStream::new(), &function)
                .unwrap_err()
                .to_string(),
            "a #[haze::resource] function returns a concrete type, not `impl Trait`"
        );
    }

    #[test]
    fn rejects_a_function_that_takes_its_own_type() {
        assert_eq!(
            Expansion::of(quote! { fn open(store: Store) -> Result<Store> { todo!() } })
                .unwrap_err(),
            "a #[haze::resource] function cannot take the type it provides"
        );
    }

    #[test]
    fn rejects_a_generic_function() {
        assert_eq!(
            Expansion::of(quote! { fn open<T>() -> T { todo!() } }).unwrap_err(),
            "a #[haze::resource] function is not generic"
        );
    }

    #[test]
    fn rejects_arguments() {
        let function = parse2::<ItemFn>(quote! { fn open() -> Store { todo!() } }).unwrap();
        let error = Resource::expand(quote! { order = 1 }, &function).unwrap_err();
        assert_eq!(
            error.to_string(),
            "a #[haze::resource] attribute takes no arguments"
        );
    }
}
