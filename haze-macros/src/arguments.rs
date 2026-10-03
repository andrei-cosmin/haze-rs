//! The arguments of `#[haze::server]` and the route attributes, read with the
//! grammar of the Dioxus attribute each one passes through to.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    Error, ExprTuple, FnArg, Ident, LitBool, LitStr, Path, Token, Type,
    buffer::Cursor,
    parse::{Lookahead1, Parse, ParseStream},
    token::{Brace, Comma},
};

use crate::handle::Handle;

/// Reads one option value and returns it as written.
type Value = fn(ParseStream<'_>) -> Result<TokenStream, Error>;

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

    /// The arguments of `#[haze::server]`, read like Dioxus's `ServerFnArgs`:
    /// up to four positional arguments (a name, then the `prefix`, `encoding`
    /// and `endpoint` strings) or `key = value` options, then the handle
    /// pairs, which start at the first `name:` and come last. The encoding is
    /// checked once every argument was read, as Dioxus checks it, so every
    /// input Dioxus rejects is rejected with Dioxus's own message.
    pub fn server(stream: ParseStream<'_>) -> Result<Self, Error> {
        let mut leading = Vec::new();
        let mut seen = Vec::new();
        let mut encoding: Option<LitStr> = None;
        let mut use_key_and_value = false;
        let mut arg_pos = 0;
        while !stream.is_empty() {
            if stream.peek(Ident) && stream.peek2(Token![:]) {
                break;
            }
            arg_pos += 1;
            let lookahead = stream.lookahead1();
            if lookahead.peek(Ident) {
                let key_or_value = stream.parse::<Ident>()?;
                let lookahead = stream.lookahead1();
                if lookahead.peek(Token![=]) {
                    let equals = stream.parse::<Token![=]>()?;
                    let key = key_or_value;
                    use_key_and_value = true;
                    let value = Self::option(&key, lookahead, &seen)?(stream)?;
                    if key == "encoding" {
                        encoding = Some(syn::parse2(value.clone())?);
                    }
                    seen.push(key.to_string());
                    leading.push(quote! { #key #equals #value });
                } else {
                    let value = key_or_value;
                    if use_key_and_value {
                        return Err(Error::new(
                            value.span(),
                            "positional argument follows keyword argument",
                        ));
                    }
                    if arg_pos != 1 {
                        return Err(Error::new(value.span(), "expected string literal"));
                    }
                    seen.push(String::from("name"));
                    leading.push(value.into_token_stream());
                }
            } else if lookahead.peek(LitStr) {
                if use_key_and_value {
                    return Err(Error::new(
                        stream.span(),
                        "If you use keyword arguments (e.g., `name` = Something), then you can no longer use arguments without a keyword.",
                    ));
                }
                let key = match arg_pos {
                    1 => return Err(lookahead.error()),
                    2 => "prefix",
                    3 => "encoding",
                    4 => "endpoint",
                    _ => return Err(Error::new(stream.span(), "unexpected extra argument")),
                };
                let value = Self::value::<LitStr>(stream)?;
                if key == "encoding" {
                    encoding = Some(syn::parse2(value.clone())?);
                }
                seen.push(String::from(key));
                leading.push(value);
            } else {
                return Err(lookahead.error());
            }
            if !stream.is_empty() {
                stream.parse::<Comma>()?;
            }
        }
        let mut handles = Vec::new();
        while stream.peek(Ident) && stream.peek2(Token![:]) {
            handles.push(Handle::of(stream.parse::<FnArg>()?)?);
            if !stream.peek(Comma) {
                break;
            }
            stream.parse::<Comma>()?;
        }
        if let Some(encoding) = encoding {
            match encoding.value().to_lowercase().as_str() {
                "url" | "cbor" | "getcbor" | "getjson" => {}
                _ => return Err(Error::new(encoding.span(), "Encoding not found.")),
            }
        }
        Ok(Self { leading, handles })
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

    /// The reader for the value of `key`, after checking that `key` is one of
    /// Dioxus's options, that it was not given before and that `input` or
    /// `output` does not follow `encoding`.
    fn option(key: &Ident, lookahead: Lookahead1<'_>, seen: &[String]) -> Result<Value, Error> {
        let value: Value = match key.to_string().as_str() {
            "name" => Self::value::<Ident>,
            "prefix" | "encoding" | "endpoint" => Self::value::<LitStr>,
            "input" | "output" | "server" | "client" | "protocol" => Self::value::<Type>,
            "input_derive" => Self::value::<ExprTuple>,
            "custom" => Self::value::<Path>,
            "impl_from" | "impl_deref" => Self::value::<LitBool>,
            _ => return Err(lookahead.error()),
        };
        if (key == "input" || key == "output") && seen.iter().any(|name| name == "encoding") {
            return Err(Error::new(
                key.span(),
                format!("`encoding` and `{key}` should not both be specified"),
            ));
        }
        if seen.iter().any(|name| key == name) {
            return Err(Error::new(
                key.span(),
                format!("keyword argument repeated: `{key}`"),
            ));
        }
        Ok(value)
    }

    /// Runs the parser of `T` and returns the tokens it read, exactly as
    /// written.
    fn value<T: Parse>(input: ParseStream<'_>) -> Result<TokenStream, Error> {
        let begin = input.cursor();
        input.parse::<T>()?;
        let end = input.cursor();
        Ok(Self::tokens_between(begin, end))
    }

    /// The tokens from `begin` up to `end`.
    fn tokens_between(begin: Cursor<'_>, end: Cursor<'_>) -> TokenStream {
        assert!(begin <= end);

        let mut cursor = begin;
        let mut tokens = TokenStream::new();
        while cursor < end {
            let (token, next) = cursor.token_tree().unwrap();
            tokens.extend(core::iter::once(token));
            cursor = next;
        }
        tokens
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
