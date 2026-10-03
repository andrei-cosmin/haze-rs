//! The `#[haze::server]` and route attribute expansion.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    Error, FnArg, Ident, ItemFn,
    parse::{ParseStream, Parser},
};

use crate::arguments::Arguments;

/// Reads the arguments of one server function attribute.
type Grammar = fn(ParseStream<'_>) -> Result<Arguments, Error>;

/// Expands a server function attribute into two copies of the function, one
/// for each way the application runs, selected by the application's own
/// `standalone` feature the way Dioxus selects its client and server code by
/// the application's `server` feature.
pub struct ServerFn;

impl ServerFn {
    /// Runs the attribute named `attribute`, whose arguments `grammar` reads,
    /// on `item`. A parse or expansion error keeps the item and appends the
    /// error, like Dioxus's `server` and `wrapped_route_impl`.
    pub fn attribute(
        arguments: proc_macro::TokenStream,
        item: proc_macro::TokenStream,
        attribute: &str,
        grammar: Grammar,
    ) -> proc_macro::TokenStream {
        let function = match syn::parse::<ItemFn>(item.clone()) {
            Ok(function) => function,
            Err(error) => {
                let mut tokens = item;
                tokens.extend(proc_macro::TokenStream::from(error.to_compile_error()));
                return tokens;
            }
        };
        match Self::expand(arguments.into(), &function, attribute, grammar) {
            Ok(tokens) => tokens.into(),
            Err(error) => {
                let mut tokens = function.to_token_stream();
                tokens.extend(error.to_compile_error());
                tokens.into()
            }
        }
    }

    /// Without the `standalone` feature, puts Dioxus's attribute of the same
    /// name on the function, with every handle handed over as `Res<T>` and
    /// opened again at the top of the body. With it, keeps the function
    /// without its `#[middleware]` attributes, whose body first reads the
    /// process default `Resources` and fetches every handle from it, so the
    /// call runs in process. Next to both copies, a `cfg` on the `standalone`
    /// feature under `deny(unexpected_cfgs)` fails the build of a crate that
    /// declares no such feature, with a reason that names the fix, the way
    /// Dioxus asks for its `server` feature. Returns a compile error for
    /// arguments Dioxus rejects, a handle that is not `name: Type`, a function
    /// that is not `async`, or a method receiver.
    pub fn expand(
        arguments: TokenStream,
        function: &ItemFn,
        attribute: &str,
        grammar: Grammar,
    ) -> Result<TokenStream, Error> {
        let Arguments { leading, handles } = grammar.parse2(arguments)?;
        if function.sig.asyncness.is_none() {
            return Err(Error::new_spanned(
                function.sig.fn_token,
                format!("a #[haze::{attribute}] function is async"),
            ));
        }
        for input in &function.sig.inputs {
            if let FnArg::Receiver(receiver) = input {
                return Err(Error::new_spanned(receiver, "Self type is not supported"));
            }
        }
        let dioxus = Ident::new(attribute, Span::call_site());
        let resources = quote!(__haze_resources);
        let mut pairs = Vec::new();
        let mut openings = Vec::new();
        let mut fetches = Vec::new();
        for handle in &handles {
            pairs.push(handle.pair());
            openings.push(handle.open());
            fetches.push(handle.fetch(&resources));
        }
        let attrs = &function.attrs;
        let vis = &function.vis;
        let sig = &function.sig;
        let block = &function.block;
        let lookup = if handles.is_empty() {
            quote! {}
        } else {
            let name = &sig.ident;
            quote! {
                let #resources = ::core::option::Option::ok_or_else(
                    ::haze::Resources::get_default(),
                    || ::haze::__private::ServerFnError::new(::core::concat!(
                        ::core::stringify!(#name),
                        " was called, but no haze Resources are installed; call Resources::install_default, which haze::launch does, before calling it",
                    )),
                )?;
            }
        };
        let mut standalone = Vec::new();
        for attr in attrs {
            if !attr.path().is_ident("middleware") {
                standalone.push(attr);
            }
        }
        Ok(quote! {
            #[deny(
                unexpected_cfgs,
                reason = "
==========================================================================================
  Using haze server functions requires a `standalone` feature flag in your `Cargo.toml`.
  Please add the following to your `Cargo.toml`:

  ```toml
  [features]
  standalone = [\"haze/standalone\"]
  ```
==========================================================================================
                "
            )]
            const _: () = {
                #[cfg(feature = "standalone")]
                let _ = ();
            };

            #[cfg(not(feature = "standalone"))]
            #[::dioxus_fullstack::#dioxus(#(#leading,)* #(#pairs),*)]
            #(#attrs)*
            #vis #sig {
                #(#openings)*
                #block
            }

            #[cfg(feature = "standalone")]
            #(#standalone)*
            #vis #sig {
                #lookup
                #(#fetches)*
                #block
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;
    use syn::{ItemFn, parse2};

    use super::{Grammar, ServerFn};
    use crate::arguments::Arguments;

    struct Expansion;

    impl Expansion {
        fn of(
            arguments: TokenStream,
            function: TokenStream,
            attribute: &str,
            grammar: Grammar,
        ) -> Result<String, String> {
            let function = parse2::<ItemFn>(function).unwrap();
            ServerFn::expand(arguments, &function, attribute, grammar)
                .map(|tokens| tokens.to_string())
                .map_err(|error| error.to_string())
        }

        fn server(arguments: TokenStream) -> String {
            Self::of(
                arguments,
                quote! {
                    #[middleware(TimeoutLayer::new(LIMIT))]
                    #[doc = "Scores."]
                    pub async fn top(count: usize) -> Result<usize, ServerFnError> {
                        Ok(count)
                    }
                },
                "server",
                Arguments::server,
            )
            .unwrap()
        }
    }

    #[test]
    fn the_fullstack_copy_hands_every_handle_to_dioxus_as_res() {
        let expanded = Expansion::server(quote! {
            endpoint = "top", scores: Scores, motto: Option<Motto>, board: Res<Board>, keeper: Option<Res<Keeper>>
        });
        let fullstack = &expanded[expanded.find("# [cfg (not").unwrap()..];
        assert!(fullstack.starts_with(
            "# [cfg (not (feature = \"standalone\"))] # [:: dioxus_fullstack :: server (endpoint = \"top\" , scores : :: haze :: Res < Scores > , motto : :: core :: option :: Option < :: haze :: Res < Motto >> , board : Res < Board > , keeper : Option < Res < Keeper > >)] # [middleware (TimeoutLayer :: new (LIMIT))] # [doc = \"Scores.\"] pub async fn top (count : usize) -> Result < usize , ServerFnError > { let scores = scores . 0 ; let motto = motto . map (| __haze_res | __haze_res . 0) ; { Ok (count) } }"
        ));
    }

    #[test]
    fn the_standalone_copy_fetches_every_handle_from_the_process_default() {
        let expanded = Expansion::server(quote! {
            scores: Scores, motto: Option<Motto>, board: Res<Board>, keeper: Option<Res<Keeper>>
        });
        let standalone = &expanded[expanded
            .rfind("# [cfg (feature = \"standalone\")]")
            .unwrap()..];
        assert!(standalone.starts_with(
            "# [cfg (feature = \"standalone\")] # [doc = \"Scores.\"] pub async fn top (count : usize) -> Result < usize , ServerFnError > { let __haze_resources = :: core :: option :: Option :: ok_or_else (:: haze :: Resources :: get_default () , || :: haze :: __private :: ServerFnError :: new (:: core :: concat ! (:: core :: stringify ! (top) , \" was called, but no haze Resources are installed; call Resources::install_default, which haze::launch does, before calling it\" ,)) ,) ? ;"
        ));
        assert!(standalone.contains(
            "let scores = :: core :: option :: Option :: ok_or_else (:: haze :: Resources :: get :: < Scores > (__haze_resources) , || :: haze :: __private :: ServerFnError :: new (:: std :: format ! (\"{} was never inserted\" , :: core :: any :: type_name :: < Scores > () ,)) ,) ? ;"
        ));
        assert!(
            standalone.contains(
                "let motto = :: haze :: Resources :: get :: < Motto > (__haze_resources) ;"
            )
        );
        assert!(standalone.contains(
            "let board = :: haze :: Res (:: core :: option :: Option :: ok_or_else (:: haze :: Resources :: get :: < Board > (__haze_resources)"
        ));
        assert!(standalone.contains(
            "let keeper = :: core :: option :: Option :: map (:: haze :: Resources :: get :: < Keeper > (__haze_resources) , :: haze :: Res ,) ; { Ok (count) } }"
        ));
        assert!(!standalone.contains("middleware"));
    }

    #[test]
    fn a_crate_without_a_standalone_feature_is_told_to_declare_one() {
        let expanded = Expansion::server(TokenStream::new());
        assert!(expanded.starts_with(
            "# [deny (unexpected_cfgs , reason = \"\n==========================================================================================\n  Using haze server functions requires a `standalone` feature flag in your `Cargo.toml`.\n  Please add the following to your `Cargo.toml`:\n\n  ```toml\n  [features]\n  standalone = [\\\"haze/standalone\\\"]\n  ```\n==========================================================================================\n                \")] const _ : () = { # [cfg (feature = \"standalone\")] let _ = () ; } ;"
        ));
    }

    #[test]
    fn a_function_without_handles_is_kept_as_it_is_when_standalone() {
        let expanded = Expansion::server(TokenStream::new());
        assert!(expanded.contains("# [:: dioxus_fullstack :: server ()]"));
        assert!(expanded.ends_with(
            "# [cfg (feature = \"standalone\")] # [doc = \"Scores.\"] pub async fn top (count : usize) -> Result < usize , ServerFnError > { { Ok (count) } }"
        ));
    }

    #[test]
    fn a_route_attribute_passes_its_route_to_dioxus() {
        let expanded = Expansion::of(
            quote! { "/api/scores?page", mut scores: Scores },
            quote! { async fn paged(page: u32) -> Result<u32, HttpError> { Ok(page) } },
            "get",
            Arguments::route,
        )
        .unwrap();
        let fullstack = &expanded[expanded.find("# [cfg (not").unwrap()..];
        assert!(fullstack.starts_with(
            "# [cfg (not (feature = \"standalone\"))] # [:: dioxus_fullstack :: get (\"/api/scores?page\" , scores : :: haze :: Res < Scores >)] async fn paged (page : u32) -> Result < u32 , HttpError > { let mut scores = scores . 0 ; { Ok (page) } }"
        ));
        assert!(expanded.contains("let mut scores = :: core :: option :: Option :: ok_or_else"));
    }

    #[test]
    fn rejects_a_function_that_is_not_async() {
        assert_eq!(
            Expansion::of(
                quote! { "/api/scores" },
                quote! { fn top() -> Result<(), ServerFnError> { Ok(()) } },
                "post",
                Arguments::route,
            )
            .unwrap_err(),
            "a #[haze::post] function is async"
        );
    }

    #[test]
    fn rejects_a_method() {
        assert_eq!(
            Expansion::of(
                TokenStream::new(),
                quote! { async fn top(&self) -> Result<(), ServerFnError> { Ok(()) } },
                "server",
                Arguments::server,
            )
            .unwrap_err(),
            "Self type is not supported"
        );
    }

    #[test]
    fn rejects_arguments_dioxus_rejects() {
        assert_eq!(
            Expansion::of(
                quote! { endpoint = "a", endpoint = "b" },
                quote! { async fn top() -> Result<(), ServerFnError> { Ok(()) } },
                "server",
                Arguments::server,
            )
            .unwrap_err(),
            "keyword argument repeated: `endpoint`"
        );
    }
}
