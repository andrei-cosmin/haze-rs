//! The `#[haze::register]` expansion.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Error, ItemImpl, LitInt, meta, parse::Parser, spanned::Spanned};

/// Expands `#[haze::register(order = N)]` on an `impl Trait for Type` into an
/// inventory `Registration` and an `Installer` that collects `dyn Trait` at
/// startup.
pub struct Register;

impl Register {
    /// Keeps the impl and submits a `Registration` that checks for and obtains
    /// `Type`, built or cloned, as `Box<dyn Trait>`, and an `Installer` that
    /// collects `dyn Trait`, or returns a compile error for a missing, unknown,
    /// repeated or invalid `order` argument, an inherent impl or a generic impl.
    pub fn expand(attribute: TokenStream, implementation: &ItemImpl) -> Result<TokenStream, Error> {
        let order = Self::order(attribute)?;
        let Some((_, trait_path, _)) = &implementation.trait_ else {
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
            #implementation

            ::haze::__private::inventory::submit! {
                ::haze::__private::Registration::new(
                    ::core::any::TypeId::of::<dyn #trait_path>,
                    #order,
                    ::core::any::type_name::<#self_type>,
                    |__haze_resources| {
                        #resolver
                        (&__haze_resolver).haze_check(__haze_resources)
                    },
                    |__haze_resources| {
                        #resolver
                        let __haze_built = (&__haze_resolver).haze_obtain(__haze_resources)?;
                        let __haze_boxed: ::std::boxed::Box<dyn #trait_path> =
                            ::std::boxed::Box::new(__haze_built);
                        ::core::result::Result::Ok(
                            ::std::boxed::Box::new(__haze_boxed)
                                as ::std::boxed::Box<dyn ::core::any::Any>
                        )
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
