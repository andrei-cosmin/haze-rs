//! The options of `#[haze::server]`, read with the grammar of Dioxus's
//! `ServerFnArgs`.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    Error, ExprTuple, Ident, LitBool, LitStr, Path, Token, Type,
    buffer::Cursor,
    parse::{Lookahead1, Parse, ParseStream},
    token::Comma,
};

/// Reads one option value and returns it as written.
type Value = fn(ParseStream<'_>) -> Result<TokenStream, Error>;

/// The options of `#[haze::server]` read so far, each recorded once with its
/// tokens as written.
#[derive(Default)]
pub struct ServerOptions {
    /// Every option as written, in order.
    pub tokens: Vec<TokenStream>,
    /// The names of the options already given.
    seen: Vec<String>,
    /// The `encoding` option, checked once every argument was read.
    encoding: Option<LitStr>,
    /// Whether a `key = value` option was given.
    keyed: bool,
    /// The position of the option being read, counted from 1.
    position: usize,
}

impl ServerOptions {
    /// Reads the options up to the first handle, which starts with `name:`.
    pub fn read(stream: ParseStream<'_>) -> Result<Self, Error> {
        let mut options = Self::default();
        while !stream.is_empty() {
            if stream.peek(Ident) && stream.peek2(Token![:]) {
                break;
            }
            options.position += 1;
            options.read_one(stream)?;
            if !stream.is_empty() {
                stream.parse::<Comma>()?;
            }
        }
        Ok(options)
    }

    /// Rejects an `encoding` that Dioxus does not know, with Dioxus's message.
    pub fn check_encoding(&self) -> Result<(), Error> {
        let Some(encoding) = &self.encoding else {
            return Ok(());
        };
        match encoding.value().to_lowercase().as_str() {
            "url" | "cbor" | "getcbor" | "getjson" => Ok(()),
            _ => Err(Error::new(encoding.span(), "Encoding not found.")),
        }
    }

    /// Reads one option: `key = value`, a bare name or a positional string.
    fn read_one(&mut self, stream: ParseStream<'_>) -> Result<(), Error> {
        let lookahead = stream.lookahead1();
        if lookahead.peek(Ident) {
            let ident = stream.parse::<Ident>()?;
            let lookahead = stream.lookahead1();
            if lookahead.peek(Token![=]) {
                self.read_keyed(&ident, lookahead, stream)
            } else {
                self.read_name(ident)
            }
        } else if lookahead.peek(LitStr) {
            self.read_positional(lookahead, stream)
        } else {
            Err(lookahead.error())
        }
    }

    /// Reads `key = value`.
    fn read_keyed(
        &mut self,
        key: &Ident,
        lookahead: Lookahead1<'_>,
        stream: ParseStream<'_>,
    ) -> Result<(), Error> {
        let equals = stream.parse::<Token![=]>()?;
        self.keyed = true;
        let value = self.reader(key, lookahead)?(stream)?;
        self.keep_encoding(&key.to_string(), &value)?;
        self.record(&key.to_string(), quote! { #key #equals #value });
        Ok(())
    }

    /// Reads the server function's name, given as the first bare identifier.
    fn read_name(&mut self, name: Ident) -> Result<(), Error> {
        if self.keyed {
            return Err(Error::new(
                name.span(),
                "positional argument follows keyword argument",
            ));
        }
        if self.position != 1 {
            return Err(Error::new(name.span(), "expected string literal"));
        }
        self.record("name", name.into_token_stream());
        Ok(())
    }

    /// Reads a positional string: the prefix, the encoding or the endpoint.
    fn read_positional(
        &mut self,
        lookahead: Lookahead1<'_>,
        stream: ParseStream<'_>,
    ) -> Result<(), Error> {
        if self.keyed {
            return Err(Error::new(
                stream.span(),
                "If you use keyword arguments (e.g., `name` = Something), then you can no longer use arguments without a keyword.",
            ));
        }
        let name = match self.position {
            1 => return Err(lookahead.error()),
            2 => "prefix",
            3 => "encoding",
            4 => "endpoint",
            _ => return Err(Error::new(stream.span(), "unexpected extra argument")),
        };
        let value = Self::value::<LitStr>(stream)?;
        self.keep_encoding(name, &value)?;
        self.record(name, value);
        Ok(())
    }

    /// The reader for the value of `key`, after checking that `key` is one of
    /// Dioxus's options, that it was not given before and that `input` or
    /// `output` does not follow `encoding`.
    fn reader(&self, key: &Ident, lookahead: Lookahead1<'_>) -> Result<Value, Error> {
        let value: Value = match key.to_string().as_str() {
            "name" => Self::value::<Ident>,
            "prefix" | "encoding" | "endpoint" => Self::value::<LitStr>,
            "input" | "output" | "server" | "client" | "protocol" => Self::value::<Type>,
            "input_derive" => Self::value::<ExprTuple>,
            "custom" => Self::value::<Path>,
            "impl_from" | "impl_deref" => Self::value::<LitBool>,
            _ => return Err(lookahead.error()),
        };
        if (key == "input" || key == "output") && self.was_given("encoding") {
            return Err(Error::new(
                key.span(),
                format!("`encoding` and `{key}` should not both be specified"),
            ));
        }
        if self.was_given(&key.to_string()) {
            return Err(Error::new(
                key.span(),
                format!("keyword argument repeated: `{key}`"),
            ));
        }
        Ok(value)
    }

    /// Whether the option `name` was already given.
    fn was_given(&self, name: &str) -> bool {
        self.seen.iter().any(|seen| seen == name)
    }

    /// Keeps the value of the `encoding` option for [`Self::check_encoding`].
    fn keep_encoding(&mut self, name: &str, value: &TokenStream) -> Result<(), Error> {
        if name == "encoding" {
            self.encoding = Some(syn::parse2(value.clone())?);
        }
        Ok(())
    }

    /// Records the option `name`, written as `tokens`.
    fn record(&mut self, name: &str, tokens: TokenStream) {
        self.seen.push(name.to_owned());
        self.tokens.push(tokens);
    }

    /// Runs the parser of `T` and returns the tokens it read, exactly as
    /// written.
    fn value<T: Parse>(input: ParseStream<'_>) -> Result<TokenStream, Error> {
        let begin = input.cursor();
        input.parse::<T>()?;
        Ok(Self::tokens_between(begin, input.cursor()))
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
