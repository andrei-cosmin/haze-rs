//! The field kinds a `#[derive(Pack)]` struct accepts.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Error, ExprCall, Ident, Path, Token, parse::ParseStream, spanned::Spanned};

use crate::parameter::Parameter;

/// One field of a `#[derive(Pack)]` struct: a resource fetched from the
/// registry, in the forms a `#[haze::resource]` parameter takes, or a value
/// computed by a `#[pack(func = ..)]` call, the way summer's `InjectableType`
/// tells injected fields from `FuncCall` fields.
pub enum Field<'a> {
    /// A resource read when the pack is built: `T` waits until `T` exists,
    /// `Option<T>` is `None` when no `T` exists at that moment, `Later<T>` is
    /// filled by the end of startup.
    Fetched(Parameter<'a>),
    /// `#[pack(func = call(..))]`: the call's result, computed once every other
    /// field was fetched, with those fields and the `func` fields above it in
    /// scope.
    Func(ExprCall),
}

impl<'a> Field<'a> {
    /// Classifies a field: a `#[pack(func = ..)]` attribute wins, like summer's
    /// `#[inject(func = ..)]`; otherwise [`Parameter::of`] classifies the type
    /// with the pack's own wording, so a field and a `#[haze::resource]`
    /// parameter accept exactly the same forms.
    pub fn of(field: &'a syn::Field) -> Result<Self, Error> {
        let mut call = None;
        for attribute in &field.attrs {
            if !attribute.path().is_ident("pack") {
                continue;
            }
            if call.is_some() {
                return Err(Error::new_spanned(attribute, "duplicate #[pack] attribute"));
            }
            call = Some(attribute.parse_args_with(Self::func)?);
        }
        if let Some(call) = call {
            return Ok(Self::Func(call));
        }
        Ok(Self::Fetched(Parameter::of(
            &field.ty,
            "a #[derive(Pack)] field",
        )?))
    }

    /// Whether this field is computed by a `#[pack(func = ..)]` call, so it is
    /// bound after every other field, like summer's `FuncCall` order.
    pub fn is_func(&self) -> bool {
        matches!(self, Self::Func(_))
    }

    /// Parses `func = call(..)`, the only `#[pack]` argument.
    fn func(input: ParseStream<'_>) -> Result<ExprCall, Error> {
        let name = input.parse::<Path>()?;
        if !name.is_ident("func") {
            return Err(Error::new_spanned(
                name,
                "expected `#[pack(func = call(..))]`",
            ));
        }
        input.parse::<Token![=]>()?;
        input.parse::<ExprCall>()
    }

    /// The statement that binds `name` inside the generated `Pack` impl,
    /// spanned on the type or the call so an error is reported on the field,
    /// like axum-macros' `FromRequest` derive. The registry calls are path
    /// calls like serde's generated code, and the registry parameter carries a
    /// generated name so a field named `resources` is not shadowed, the way
    /// serde prefixes its generated locals; it is interpolated from a plain
    /// `quote!` and keeps the derive's call-site span, because spanned on a
    /// type that came through a `macro_rules!` body it would resolve in that
    /// body's hygiene context and not be found, which is why serde interpolates
    /// `__deserializer` into its spanned tokens.
    pub fn binding(&self, name: &Ident) -> TokenStream {
        let resources = quote!(__haze_resources);
        match self {
            Self::Fetched(Parameter::Required(ty)) => quote_spanned! {ty.span()=>
                let #name = ::core::option::Option::ok_or_else(
                    ::haze::Resources::get::<#ty>(#resources),
                    || ::haze::__private::anyhow::anyhow!(
                        "{} needs {}, which was never inserted",
                        ::core::any::type_name::<Self>(),
                        ::core::any::type_name::<#ty>(),
                    ),
                )?;
            },
            Self::Fetched(Parameter::Optional(inner)) => quote_spanned! {inner.span()=>
                let #name = ::haze::Resources::get::<#inner>(#resources);
            },
            Self::Fetched(Parameter::Later(inner)) => quote_spanned! {inner.span()=>
                let #name = ::haze::Resources::later::<#inner>(#resources);
            },
            Self::Func(call) => quote_spanned! {call.span()=>
                let #name = #call;
            },
        }
    }
}
