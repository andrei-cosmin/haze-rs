//! The arguments of `#[haze::server]` and the route attributes, read with the
//! grammar of the Dioxus attribute each one passes through to.

use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use syn::{
    Error, FnArg, Ident, LitStr, Token,
    parse::{Parse, ParseStream},
    token::{Brace, Comma},
};

use crate::handle::Handle;
use crate::server_options::ServerOptions;

/// The arguments of a server function attribute: what Dioxus reads itself,
/// kept as written, then the handles, the way Dioxus's `ServerFnArgs` and
/// `Route` read their options or route first and the server-only arguments
/// last.
pub struct Arguments {
    /// The options of `#[haze::server]`, or the route of a route attribute,
    /// each as written, in order.
    pub leading: Vec<TokenStream>,
    /// The handles, in order.
    pub handles: Vec<Handle>,
}

impl Arguments {
    /// The methods Dioxus's route grammar accepts before the route.
    const METHODS: [&str; 8] = [
        "GET", "POST", "PUT", "DELETE", "HEAD", "CONNECT", "OPTIONS", "TRACE",
    ];

    /// The arguments of `#[haze::server]`: Dioxus's options, then the handle
    /// pairs, which start at the first `name:`. The encoding is checked last,
    /// as Dioxus checks it.
    pub fn server(stream: ParseStream<'_>) -> Result<Self, Error> {
        let options = ServerOptions::read(stream)?;
        let mut handles = Vec::new();
        while stream.peek(Ident) && stream.peek2(Token![:]) {
            handles.push(Handle::of(stream.parse::<FnArg>()?)?);
            if !stream.peek(Comma) {
                break;
            }
            stream.parse::<Comma>()?;
        }
        options.check_encoding()?;
        Ok(Self {
            leading: options.tokens,
            handles,
        })
    }

    /// The arguments of a route attribute, read like Dioxus's `Route`: an
    /// optional method, the route string, then `, ` and the handle pairs. A
    /// braced `OpenAPI` block, which these attributes never accept, is
    /// rejected first, then a method, which comes from the attribute, both
    /// with Dioxus's own message and in Dioxus's order.
    pub fn route(input: ParseStream<'_>) -> Result<Self, Error> {
        let method = if input.peek(Ident) {
            let method = input.parse::<Ident>()?;
            if !Self::METHODS.contains(&method.to_string().to_uppercase().as_str()) {
                return Err(input.error(
                    "expected one of (GET, POST, PUT, DELETE, HEAD, CONNECT, OPTIONS, TRACE)",
                ));
            }
            Some(method)
        } else {
            None
        };
        let route_lit = input.parse::<LitStr>()?;
        if input.peek(Brace) {
            return Err(Error::new(
                Span::call_site(),
                "Use `api_route` instead of `route` to use OpenAPI options",
            ));
        }
        if method.is_some() {
            return Err(Error::new(
                Span::call_site(),
                "HTTP method specified both in macro and in attribute",
            ));
        }
        let mut handles = Vec::new();
        if input.peek(Comma) {
            input.parse::<Comma>()?;
            for argument in input.parse_terminated(FnArg::parse, Comma)? {
                handles.push(Handle::of(argument)?);
            }
        }
        Ok(Self {
            leading: vec![route_lit.into_token_stream()],
            handles,
        })
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;
    use syn::{Error, parse::Parser};

    use super::Arguments;

    struct Parsed;

    impl Parsed {
        fn server(arguments: TokenStream) -> Result<(Vec<String>, Vec<String>), String> {
            Self::split(Arguments::server.parse2(arguments))
        }

        fn route(arguments: TokenStream) -> Result<(Vec<String>, Vec<String>), String> {
            Self::split(Arguments::route.parse2(arguments))
        }

        fn split(parsed: Result<Arguments, Error>) -> Result<(Vec<String>, Vec<String>), String> {
            let arguments = parsed.map_err(|error| error.to_string())?;
            let mut leading = Vec::new();
            for tokens in &arguments.leading {
                leading.push(tokens.to_string());
            }
            let mut pairs = Vec::new();
            for handle in &arguments.handles {
                pairs.push(handle.pair().to_string());
            }
            Ok((leading, pairs))
        }
    }

    #[test]
    fn options_come_first_and_handles_last() {
        let (leading, pairs) = Parsed::server(quote! {
            input = Json<A, B>, endpoint = "scores", scores: Scores, motto: Option<Motto>
        })
        .unwrap();
        assert_eq!(leading, ["input = Json < A , B >", "endpoint = \"scores\""]);
        assert_eq!(
            pairs,
            [
                "scores : :: haze :: Res < Scores >",
                "motto : :: core :: option :: Option < :: haze :: Res < Motto >>"
            ]
        );
    }

    #[test]
    fn the_legacy_positional_arguments_are_kept() {
        let (leading, pairs) =
            Parsed::server(quote! { Scores, "/api", "Url", "scores", scores: Scores }).unwrap();
        assert_eq!(leading, ["Scores", "\"/api\"", "\"Url\"", "\"scores\""]);
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn handles_alone_and_nothing_at_all_are_accepted() {
        assert_eq!(
            Parsed::server(quote! { scores: Scores, }).unwrap().1.len(),
            1
        );
        assert_eq!(
            Parsed::server(TokenStream::new()).unwrap(),
            (Vec::new(), Vec::new())
        );
    }

    #[test]
    fn server_options_are_rejected_the_way_dioxus_rejects_them() {
        for (arguments, message) in [
            (
                quote! { endpoint = "a", endpoint = "b" },
                "keyword argument repeated: `endpoint`",
            ),
            (
                quote! { Scores, name = Board },
                "keyword argument repeated: `name`",
            ),
            (
                quote! { encoding = "url", input = Json },
                "`encoding` and `input` should not both be specified",
            ),
            (
                quote! { encoding = "url", output = Json },
                "`encoding` and `output` should not both be specified",
            ),
            (quote! { route = "/a" }, "unexpected token"),
            (
                quote! { endpoint = "a", Scores },
                "positional argument follows keyword argument",
            ),
            (quote! { Scores, Board }, "expected string literal"),
            (quote! { "/api" }, "expected identifier"),
            (
                quote! { endpoint = "a", "/api" },
                "If you use keyword arguments (e.g., `name` = Something), then you can no longer use arguments without a keyword.",
            ),
            (
                quote! { Scores, "/api", "url", "scores", "extra" },
                "unexpected extra argument",
            ),
            (quote! { encoding = "yaml" }, "Encoding not found."),
            (quote! { Scores, "/api", "yaml" }, "Encoding not found."),
            (
                quote! { encoding = "yaml", input = Json },
                "`encoding` and `input` should not both be specified",
            ),
            (
                quote! { Scores, "/api", "yaml", "x", "extra" },
                "unexpected extra argument",
            ),
            (quote! { impl_from = yes }, "expected boolean literal"),
            (quote! { scores: Scores endpoint }, "unexpected token"),
        ] {
            assert_eq!(Parsed::server(arguments).unwrap_err(), message);
        }
    }

    #[test]
    fn a_route_is_kept_with_its_handles() {
        let (leading, pairs) = Parsed::route(
            quote! { "/api/scores/{id}?page", mut scores: Scores, motto: Res<Motto> },
        )
        .unwrap();
        assert_eq!(leading, ["\"/api/scores/{id}?page\""]);
        assert_eq!(
            pairs,
            [
                "scores : :: haze :: Res < Scores >",
                "motto : Res < Motto >"
            ]
        );
        assert_eq!(
            Parsed::route(quote! { "/api/scores", }).unwrap(),
            (vec![String::from("\"/api/scores\"")], Vec::new())
        );
    }

    #[test]
    fn routes_are_rejected_the_way_dioxus_rejects_them() {
        for (arguments, message) in [
            (
                TokenStream::new(),
                "unexpected end of input, expected string literal",
            ),
            (
                quote! { GET "/api/scores" },
                "HTTP method specified both in macro and in attribute",
            ),
            (
                quote! { fetch "/api/scores" },
                "expected one of (GET, POST, PUT, DELETE, HEAD, CONNECT, OPTIONS, TRACE)",
            ),
            (
                quote! { "/api/scores" { summary: "Scores" } },
                "Use `api_route` instead of `route` to use OpenAPI options",
            ),
            (
                quote! { GET "/api/scores" { summary: "Scores" } },
                "Use `api_route` instead of `route` to use OpenAPI options",
            ),
            (quote! { "/api/scores" scores: Scores }, "unexpected token"),
        ] {
            assert_eq!(Parsed::route(arguments).unwrap_err(), message);
        }
    }

    #[test]
    fn a_handle_is_a_plain_name_and_a_type() {
        for (arguments, message) in [
            (quote! { "/a", self }, "Self type is not supported"),
            (
                quote! { "/a", (left, right): Pair },
                "a server function handle is a name; destructure it inside the function body",
            ),
            (
                quote! { "/a", ref scores: Scores },
                "a server function handle does not support `ref` or subpatterns",
            ),
            (
                quote! { "/a", scores @ _: Scores },
                "a server function handle does not support `ref` or subpatterns",
            ),
        ] {
            assert_eq!(Parsed::route(arguments).unwrap_err(), message);
        }
    }
}
