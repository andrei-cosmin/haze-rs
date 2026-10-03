//! The validated signature of a `#[haze::resource]` function.

use quote::ToTokens;
use syn::{Error, FnArg, Ident, ItemFn, ReturnType, Type};

use crate::parameter::Parameter;

/// A `#[haze::resource]` function parsed and checked once, the way summer's
/// `Service::new` and serde's `Container::from_ast` parse their input before
/// generating code.
pub struct Signature<'a> {
    /// The function's name.
    pub name: &'a Ident,
    /// The type it inserts: the return type, or `T` from `Result<T, E>`.
    pub provided: &'a Type,
    /// Whether it returns a `Result`.
    pub fallible: bool,
    /// Whether it is `async`.
    pub asynchronous: bool,
    /// Its parameters, in order.
    pub parameters: Vec<Parameter<'a>>,
}

impl<'a> Signature<'a> {
    /// Parses and validates the function, or returns a compile error pointing
    /// at the part that is not allowed.
    pub fn parse(function: &'a ItemFn) -> Result<Self, Error> {
        if !function.sig.generics.params.is_empty() {
            return Err(Error::new_spanned(
                &function.sig.generics,
                "a #[haze::resource] function is not generic",
            ));
        }
        let ReturnType::Type(_, returned) = &function.sig.output else {
            return Err(Error::new_spanned(
                &function.sig,
                "a #[haze::resource] function returns the resource it provides",
            ));
        };
        let fallible = Parameter::wrapped(returned, "Result");
        let provided = fallible.unwrap_or(returned);
        if let Type::ImplTrait(_) = Parameter::ungroup(provided) {
            return Err(Error::new_spanned(
                provided,
                "a #[haze::resource] function returns a concrete type, not `impl Trait`",
            ));
        }
        if Parameter::wrapped(provided, "Res").is_some() {
            return Err(Error::new_spanned(
                provided,
                "a #[haze::resource] function returns `T`, not `Res<T>`; `Res` is for server functions",
            ));
        }
        if Parameter::wrapped(provided, "Later").is_some() {
            return Err(Error::new_spanned(
                provided,
                "a #[haze::resource] function returns `T`, not `Later<T>`; a function that needs a cycle takes `Later<T>`",
            ));
        }
        if Parameter::wrapped(provided, "Option").is_some() {
            return Err(Error::new_spanned(
                provided,
                "a #[haze::resource] function returns `T`, not `Option<T>`; a function that can do without it takes `Option<T>`",
            ));
        }
        let mut parameters = Vec::new();
        for input in &function.sig.inputs {
            let FnArg::Typed(typed) = input else {
                return Err(Error::new_spanned(
                    input,
                    "a #[haze::resource] function is a free function, not a method",
                ));
            };
            let parameter = Parameter::of(&typed.ty, "a #[haze::resource] parameter")?;
            if parameter.inner().to_token_stream().to_string()
                == provided.to_token_stream().to_string()
            {
                return Err(Error::new_spanned(
                    &typed.ty,
                    "a #[haze::resource] function cannot take the type it provides",
                ));
            }
            parameters.push(parameter);
        }
        Ok(Self {
            name: &function.sig.ident,
            provided,
            fallible: fallible.is_some(),
            asynchronous: function.sig.asyncness.is_some(),
            parameters,
        })
    }
}
