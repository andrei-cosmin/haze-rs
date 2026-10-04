//! The `#[haze::register]` expansion.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Error, ItemImpl, LitInt, meta, parse::Parser, spanned::Spanned};

/// Expands `#[haze::register(order = N)]` on an `impl Trait for Type` into an
/// inventory `Registration` and an `Installer` that collects `dyn Trait` at
/// startup.
pub struct Register;

impl Register {
    pub fn attribute(
        attribute: proc_macro::TokenStream,
        item: proc_macro::TokenStream,
    ) -> proc_macro::TokenStream {
        let generated = syn::parse::<ItemImpl>(item.clone())
            .and_then(|implementation| Self::expand(attribute.into(), &implementation))
            .unwrap_or_else(|error| error.to_compile_error());
        let mut tokens = item;
        tokens.extend(proc_macro::TokenStream::from(generated));
        tokens
    }

    /// The two items submitted for the impl: a `Registration` that obtains
    /// `Type`, built or cloned, as `Box<dyn Trait>`, and an `Installer` that
    /// collects `dyn Trait`.
    ///
    /// Returns a compile error for a missing, unknown, repeated or invalid
    /// `order` argument, an inherent impl or a generic impl.
    fn expand(attribute: TokenStream, implementation: &ItemImpl) -> Result<TokenStream, Error> {
        let order = Self::order(attribute)?;
        let Some((trait_path, _)) = &implementation.trait_ else {
            return Err(Error::new(
                implementation.self_ty.span(),
                "a #[haze::register] impl implements a trait: `impl Trait for Type`",
            ));
        };
        if !implementation.generics.params.is_empty() {
            return Err(Error::new_spanned(
                &implementation.generics,
                "a #[haze::register] impl is not generic",
            ));
        }
        let self_type = &implementation.self_ty;
        let collector = quote_spanned! {trait_path.span()=>
            ::haze::Resources::collect::<dyn #trait_path>
        };
        let resolver = quote! {
            use ::haze::__private::{Built as _, Inserted as _};
            let __haze_resolver = ::haze::__private::Obtain::<#self_type>(
                ::core::marker::PhantomData,
            );
        };
        Ok(quote! {
            ::haze::__private::inventory::submit! {
                ::haze::__private::Registration::new(
                    ::core::any::TypeId::of::<dyn #trait_path>,
                    #order,
                    ::core::any::type_name::<#self_type>,
                    |__haze_resources| {
                        #resolver
                        (&__haze_resolver).haze_check(__haze_resources)
                    },
                    |__haze_resources, __haze_collected| {
                        #resolver
                        let __haze_built = (&__haze_resolver).haze_obtain(__haze_resources)?;
                        let ::core::option::Option::Some(__haze_list) = __haze_collected
                            .downcast_mut::<::std::vec::Vec<::std::boxed::Box<dyn #trait_path>>>()
                        else {
                            ::core::unreachable!(
                                "{} was collected as another trait",
                                ::core::any::type_name::<#self_type>(),
                            );
                        };
                        __haze_list.push(::std::boxed::Box::new(__haze_built));
                        ::core::result::Result::Ok(())
                    },
                )
            }

            ::haze::__private::inventory::submit! {
                ::haze::__private::Installer::new(
                    ::haze::__private::Need::of::<::haze::Seq<dyn #trait_path>>(),
                    #collector,
                )
            }
        })
    }

    /// Parses the required `order = <integer>` argument as an `i32`, rejecting
    /// unknown or repeated arguments and numbers that do not fit.
    fn order(attribute: TokenStream) -> Result<i32, Error> {
        let span = attribute.span();
        let mut order = None;
        let parser = meta::parser(|item| {
            if !item.path.is_ident("order") {
                return Err(item.error("expected `order = <number>`"));
            }
            if order.is_some() {
                return Err(item.error("duplicate `order` argument"));
            }
            order = Some(item.value()?.parse::<LitInt>()?.base10_parse::<i32>()?);
            Ok(())
        });
        parser.parse2(attribute)?;
        order.ok_or_else(|| {
            Error::new(
                span,
                "a #[haze::register] attribute needs `order = <number>`",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;
    use syn::{ItemImpl, parse2};

    use super::Register;

    struct Expansion;

    impl Expansion {
        fn of(attribute: TokenStream, implementation: TokenStream) -> Result<String, String> {
            let implementation = parse2::<ItemImpl>(implementation).unwrap();
            Register::expand(attribute, &implementation)
                .map(|tokens| tokens.to_string())
                .map_err(|error| error.to_string())
        }
    }

    #[test]
    fn an_impl_submits_its_registration_and_the_collector() {
        let tokens = Expansion::of(quote! { order = 1 }, quote! { impl Port for Disk {} }).unwrap();
        assert!(tokens.contains(":: haze :: __private :: Registration :: new"));
        assert!(tokens.contains(":: haze :: __private :: Installer :: new"));
    }

    #[test]
    fn rejects_a_missing_order() {
        assert_eq!(
            Expansion::of(quote! {}, quote! { impl Port for Disk {} }).unwrap_err(),
            "a #[haze::register] attribute needs `order = <number>`"
        );
    }

    #[test]
    fn rejects_an_unknown_argument() {
        assert_eq!(
            Expansion::of(quote! { rank = 1 }, quote! { impl Port for Disk {} }).unwrap_err(),
            "expected `order = <number>`"
        );
    }

    #[test]
    fn rejects_a_repeated_order() {
        assert_eq!(
            Expansion::of(
                quote! { order = 1, order = 2 },
                quote! { impl Port for Disk {} }
            )
            .unwrap_err(),
            "duplicate `order` argument"
        );
    }

    #[test]
    fn rejects_an_order_that_does_not_fit_an_i32() {
        assert!(
            Expansion::of(
                quote! { order = 3000000000 },
                quote! { impl Port for Disk {} }
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_an_inherent_impl() {
        assert_eq!(
            Expansion::of(quote! { order = 1 }, quote! { impl Disk {} }).unwrap_err(),
            "a #[haze::register] impl implements a trait: `impl Trait for Type`"
        );
    }

    #[test]
    fn rejects_a_generic_impl() {
        assert_eq!(
            Expansion::of(quote! { order = 1 }, quote! { impl<T> Port for Disk<T> {} })
                .unwrap_err(),
            "a #[haze::register] impl is not generic"
        );
    }
}
