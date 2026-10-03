//! One resource a server function takes, written `name: Type` after the
//! attribute's options or route.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Error, FnArg, Pat, PatIdent, Type, spanned::Spanned};

use crate::form::Form;

/// A handle pair of `#[haze::server]` or a route attribute: the name the body
/// uses and the resource it receives, the way Dioxus reads each server-only
/// argument as an `FnArg`.
pub struct Handle {
    /// The name, with `mut` when the route attribute gave one.
    binding: PatIdent,
    /// The type as written.
    declared: Type,
}

impl Handle {
    /// Reads one parsed argument, or returns a compile error for `self`, a
    /// pattern that is not a plain name, or a name bound by reference or with
    /// a subpattern.
    pub fn of(argument: FnArg) -> Result<Self, Error> {
        let typed = match argument {
            FnArg::Typed(typed) => typed,
            FnArg::Receiver(receiver) => {
                return Err(Error::new_spanned(receiver, "Self type is not supported"));
            }
        };
        let Pat::Ident(binding) = *typed.pat else {
            return Err(Error::new_spanned(
                &typed.pat,
                "a server function handle is a name; destructure it inside the function body",
            ));
        };
        if binding.by_ref.is_some() || binding.subpat.is_some() {
            return Err(Error::new_spanned(
                binding,
                "a server function handle does not support `ref` or subpatterns",
            ));
        }
        Ok(Self {
            binding,
            declared: *typed.ty,
        })
    }

    /// The pair handed to Dioxus: `name: Res<T>` for `T` and
    /// `name: Option<Res<T>>` for `Option<T>`, so Dioxus extracts the
    /// resource; a `Res` form exactly as written.
    pub fn pair(&self) -> TokenStream {
        let binding = &self.binding;
        let declared = &self.declared;
        let name = &binding.ident;
        match Form::of(declared) {
            Form::Required(resource) => quote_spanned! {declared.span()=>
                #name: ::haze::Res<#resource>
            },
            Form::Optional(resource) => quote_spanned! {declared.span()=>
                #name: ::core::option::Option<::haze::Res<#resource>>
            },
            Form::Res(_) | Form::OptionalRes(_) => quote! { #binding: #declared },
        }
    }

    /// The statement that opens the `Res` Dioxus extracted, so the body sees
    /// the type as written; nothing for a `Res` form.
    pub fn open(&self) -> TokenStream {
        let binding = &self.binding;
        let name = &binding.ident;
        match Form::of(&self.declared) {
            Form::Required(_) => quote! { let #binding = #name.0; },
            Form::Optional(_) => quote! { let #binding = #name.map(|__haze_res| __haze_res.0); },
            Form::Res(_) | Form::OptionalRes(_) => quote! {},
        }
    }

    /// The statement that binds the handle from `resources`, the process
    /// default, spanned on the resource type so an error is reported on the
    /// handle. A missing `T` returns early with the `500` that the `Res`
    /// extractor's rejection becomes, naming `T`. `resources` keeps its
    /// call-site span for the reason given on `Field::binding`.
    pub fn fetch(&self, resources: &TokenStream) -> TokenStream {
        let binding = &self.binding;
        let form = Form::of(&self.declared);
        let resource = form.inner();
        let required = quote_spanned! {resource.span()=>
            ::core::option::Option::ok_or_else(
                ::haze::Resources::get::<#resource>(#resources),
                || ::haze::__private::ServerFnError::new(::std::format!(
                    "{} was never inserted",
                    ::core::any::type_name::<#resource>(),
                )),
            )?
        };
        match form {
            Form::Required(_) => quote_spanned! {resource.span()=>
                let #binding = #required;
            },
            Form::Optional(_) => quote_spanned! {resource.span()=>
                let #binding = ::haze::Resources::get::<#resource>(#resources);
            },
            Form::Res(_) => quote_spanned! {resource.span()=>
                let #binding = ::haze::Res(#required);
            },
            Form::OptionalRes(_) => quote_spanned! {resource.span()=>
                let #binding = ::core::option::Option::map(
                    ::haze::Resources::get::<#resource>(#resources),
                    ::haze::Res,
                );
            },
        }
    }
}
