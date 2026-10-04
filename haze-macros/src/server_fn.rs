//! The `#[haze::server]` and route attribute expansion.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Error, Ident, ItemFn,
    parse::{ParseStream, Parser},
};

use crate::{arguments::Arguments, handle::Handle};

/// Reads the arguments of one server function attribute.
type Grammar = fn(ParseStream<'_>) -> Result<Arguments, Error>;

/// Expands a server function attribute into Dioxus's server function with its
/// handles opened, and, when haze is built with `standalone`, a second copy
/// that runs in process, selected by the application's own `standalone`
/// feature the way Dioxus selects its client and server code by the
/// application's `server` feature.
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
        let expanded = syn::parse::<ItemFn>(item.clone())
            .and_then(|function| Self::expand(arguments.into(), &function, attribute, grammar));
        match expanded {
            Ok(tokens) => tokens.into(),
            Err(error) => {
                let mut tokens = item;
                tokens.extend(proc_macro::TokenStream::from(error.to_compile_error()));
                tokens
            }
        }
    }

    /// Emits the function the way the application runs.
    ///
    /// - Without haze's `standalone` feature: Dioxus's attribute of the same
    ///   name, with every handle passed as `Res<T>` and unwrapped at the top
    ///   of the body, and nothing else.
    /// - With it: that copy under `cfg(not(feature = "standalone"))`, and a
    ///   second copy under `cfg(feature = "standalone")` without its
    ///   `#[middleware]` attributes, whose body reads every handle from the
    ///   process default `Resources`, so the call runs in process. Next to
    ///   both, a `cfg` on `standalone` under `deny(unexpected_cfgs)` fails
    ///   the build of a crate without that feature, naming the fix, as Dioxus
    ///   does for its `server` feature.
    ///
    /// Returns a compile error for arguments Dioxus rejects, a handle that is
    /// not `name: Type`, a function that is not `async`, or a method receiver.
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
                format_args!("a #[haze::{attribute}] function is async"),
            ));
        }
        if let Some(receiver) = function.sig.receiver() {
            return Err(Error::new_spanned(receiver, "Self type is not supported"));
        }
        let dioxus = Ident::new(attribute, Span::call_site());
        let pairs = handles.iter().map(Handle::pair);
        let openings = handles.iter().map(Handle::open);
        let attrs = &function.attrs;
        let vis = &function.vis;
        let sig = &function.sig;
        let block = &function.block;
        let fullstack = quote! {
            #[::dioxus_fullstack::#dioxus(#(#leading,)* #(#pairs),*)]
            #(#attrs)*
            #vis #sig {
                #(#openings)*
                #block
            }
        };
        #[cfg(not(feature = "standalone"))]
        {
            Ok(fullstack)
        }
        #[cfg(feature = "standalone")]
        {
            let standalone = Self::standalone(&handles, function);
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
                #fullstack

                #[cfg(feature = "standalone")]
                #standalone
            })
        }
    }

    /// The in-process copy: the function without its `#[middleware]`
    /// attributes, reading every handle from the process default `Resources`.
    #[cfg(feature = "standalone")]
    fn standalone(handles: &[Handle], function: &ItemFn) -> TokenStream {
        let resources = quote!(__haze_resources);
        let fetches = handles.iter().map(|handle| handle.fetch(&resources));
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
        let attrs = function
            .attrs
            .iter()
            .filter(|attr| !attr.path().is_ident("middleware"));
        quote! {
            #(#attrs)*
            #vis #sig {
                #lookup
                #(#fetches)*
                #block
            }
        }
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
        let fullstack = &expanded[expanded.find("# [:: dioxus_fullstack").unwrap()..];
        assert!(fullstack.starts_with(
            "# [:: dioxus_fullstack :: server (endpoint = \"top\" , scores : :: haze :: Res < Scores > , motto : :: core :: option :: Option < :: haze :: Res < Motto >> , board : Res < Board > , keeper : Option < Res < Keeper > >)] # [middleware (TimeoutLayer :: new (LIMIT))] # [doc = \"Scores.\"] pub async fn top (count : usize) -> Result < usize , ServerFnError > { let scores = scores . 0 ; let motto = motto . map (| __haze_res | __haze_res . 0) ; { Ok (count) } }"
        ));
    }

    #[cfg(not(feature = "standalone"))]
    #[test]
    fn a_fullstack_only_build_gets_one_copy_and_no_cfg() {
        let expanded = Expansion::server(quote! { scores: Scores });
        assert!(
            expanded.starts_with(
                "# [:: dioxus_fullstack :: server (scores : :: haze :: Res < Scores >)]"
            )
        );
        assert!(!expanded.contains("cfg"));
        assert!(!expanded.contains("standalone"));
        assert!(expanded.contains("middleware"));
    }

    #[cfg(feature = "standalone")]
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
            "let scores = :: core :: clone :: Clone :: clone (:: core :: option :: Option :: ok_or_else (:: haze :: Resources :: get :: < Scores > (__haze_resources) , || :: haze :: __private :: ServerFnError :: new (:: core :: format_args ! (\"{} was never inserted\" , :: core :: any :: type_name :: < Scores > () ,)) ,) ?) ;"
        ));
        assert!(
            standalone.contains(
                "let motto = :: core :: option :: Option :: map (:: haze :: Resources :: get :: < Motto > (__haze_resources) , :: core :: clone :: Clone :: clone ,) ;"
            )
        );
        assert!(standalone.contains(
            "let board = :: haze :: Res (:: core :: clone :: Clone :: clone (:: core :: option :: Option :: ok_or_else (:: haze :: Resources :: get :: < Board > (__haze_resources)"
        ));
        assert!(standalone.contains(
            "let keeper = :: core :: option :: Option :: map (:: haze :: Resources :: get :: < Keeper > (__haze_resources) , | __haze_value | :: haze :: Res (:: core :: clone :: Clone :: clone (__haze_value)) ,) ; { Ok (count) } }"
        ));
        assert!(!standalone.contains("middleware"));
    }

    #[cfg(feature = "standalone")]
    #[test]
    fn a_crate_without_a_standalone_feature_is_told_to_declare_one() {
        let expanded = Expansion::server(TokenStream::new());
        assert!(expanded.starts_with(
            "# [deny (unexpected_cfgs , reason = \"\n==========================================================================================\n  Using haze server functions requires a `standalone` feature flag in your `Cargo.toml`.\n  Please add the following to your `Cargo.toml`:\n\n  ```toml\n  [features]\n  standalone = [\\\"haze/standalone\\\"]\n  ```\n==========================================================================================\n                \")] const _ : () = { # [cfg (feature = \"standalone\")] let _ = () ; } ;"
        ));
    }

    #[cfg(feature = "standalone")]
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
        let fullstack = &expanded[expanded.find("# [:: dioxus_fullstack").unwrap()..];
        assert!(fullstack.starts_with(
            "# [:: dioxus_fullstack :: get (\"/api/scores?page\" , scores : :: haze :: Res < Scores >)] async fn paged (page : u32) -> Result < u32 , HttpError > { let mut scores = scores . 0 ; { Ok (page) } }"
        ));
        #[cfg(feature = "standalone")]
        assert!(expanded.contains(
            "let mut scores = :: core :: clone :: Clone :: clone (:: core :: option :: Option :: ok_or_else"
        ));
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
