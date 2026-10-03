//! Procedural macros for `haze`.
//!
//! Use it through `haze`, which re-exports it; this crate is not meant to be a
//! direct dependency.

#![warn(missing_docs)]

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::{DeriveInput, ItemFn, ItemImpl, parse_macro_input};

mod arguments;
mod field;
mod form;
mod handle;
mod pack;
mod parameter;
mod register;
mod resource;
mod server_fn;
mod signature;

use arguments::Arguments;
use pack::Pack;
use register::Register;
use resource::Resource;
use server_fn::ServerFn;

/// Builds a struct of resources once at startup and inserts it as a resource.
///
/// A pack is built after the resource functions ran, then inserted into
/// `haze::Resources` under its own type, so server functions take it like any
/// resource, as `haze::Res<Scores>` or `Option<haze::Res<Scores>>`, and its
/// methods hold the logic they share. The struct must be `Clone`, like every
/// resource, and implements `haze::Pack`; the derive generates nothing else
/// but the record that makes startup build it, so a pack is plain Rust and
/// needs no haze feature. Each field is:
///
/// - `T`: the value inserted as exactly `T`; the pack waits until it exists.
///   A `Seq<dyn Trait>` field receives the collected implementations of that
///   trait, and another pack's type that pack.
/// - `Option<T>`: `None` if no `T` exists when the pack is built. Packs and
///   registered traits are installed in rounds, in the order of the type each
///   inserts, so an `Option` of a `Seq` or of another pack is filled only if
///   that item was installed before this pack was built; take those as `T` or
///   `Later<T>`.
/// - `Later<T>`: not waited for, filled by the end of startup; use it for a
///   cycle between two packs.
/// - `#[pack(func = call(..))]`: the call's result, computed once, after every
///   other field was fetched; the call sees those fields and the `func` fields
///   above it, and may be an associated function, as in
///   `#[pack(func = Self::open(&store))]`.
///
/// The fetched forms are the ones a `#[haze::resource]` parameter takes, with
/// the same rules: `Res<T>` is the server-function form and is rejected at the
/// top or directly inside `Option` or `Later`, as are `Option<Later<T>>` and
/// `Later<Later<T>>`; a reference field is `&'static T`. `Option`, `Later` and
/// `Res` are recognized by the last segment of the type's path, like summer's
/// wrappers, so a type of your own with one of those names needs a type alias
/// here.
///
/// Packs and registered traits are installed together, in rounds: one that
/// waits for another is built in a later round. Resource functions run before
/// all of them, so a resource function takes a pack only as `Later<T>`: as `T`
/// it stops startup, and as `Option<T>` it is always `None`.
///
/// A value of the pack's type inserted in setup or by a resource function
/// wins, and the pack is not built. When packs are left that cannot be built,
/// startup stops with an error naming each of them and the first type it is
/// missing. A pack does not implement `haze::Build`: a `#[haze::register]`ed
/// pack is cloned from the registry once startup has built it, so it is built
/// once and the `Seq` holds a clone of it, sharing its `Arc` fields.
///
/// Supports structs with named fields and no generic parameters.
///
/// # Examples
///
/// ```rust,ignore
/// #[derive(Clone, haze::Pack)]
/// struct Achievements {
///     storage: Arc<Storage>,
///     notifications: Option<Notifications>,
///     #[pack(func = Self::open_cache(&storage))]
///     cache: Cache,
/// }
///
/// #[server(achievements: haze::Res<Achievements>)]
/// async fn unlock(badge: Badge) -> Result<()> {
///     achievements.unlock(badge)
/// }
/// ```
#[proc_macro_derive(Pack, attributes(pack))]
pub fn derive_pack(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match Pack::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Registers an implementation of a trait so startup collects it into a
/// `haze::Seq<dyn Trait>`.
///
/// Apply this attribute to an `impl Trait for Type` block without generic
/// parameters. `Trait` must be dyn-compatible and have `Send + Sync` among its
/// supertraits; otherwise the attribute does not compile.
///
/// If `Type` implements `haze::Build`, collection calls that implementation.
/// Otherwise, `Type` must implement `Clone + Send + Sync + 'static` and already
/// be inserted in `haze::Resources`; a `#[derive(Pack)]` struct counts once
/// startup has built it, so the `Seq` holds a clone of the pack that
/// `Res<Pack>` hands out. `Build` takes precedence when both are available.
///
/// The required `order` argument is an `i32` integer literal. Implementations
/// are collected in ascending order, with ties broken by type name.
///
/// Startup, whether `haze::serve` runs it on a server, `haze::launch` or
/// `haze::install` in a standalone app, or `haze::Resources::start` anywhere
/// else, collects every registered trait after the resource functions ran,
/// while the packs install, and inserts the result as `Seq<dyn Trait>`, so an
/// implementation may need what a `#[haze::resource]` function provides,
/// another trait's `Seq` or a pack. Every implementation to clone is checked
/// before any `Build` runs, so a trait whose registered pack is built in a
/// later round does not build its other implementations twice. A `Build` that
/// itself waits for a later round does make the implementations before it
/// build again, and a `Build` type registered for several traits is built once
/// per trait, so keep side effects out of `build` and share state through a
/// pack or an inserted value instead. A `Seq<dyn Trait>` inserted or collected
/// by hand in setup is kept. When some cannot be obtained, startup fails
/// naming, for each trait left, one implementation and its error: the first
/// implementation to clone that was never inserted, otherwise the first
/// `Build` that fails, in collection order, including one only waiting for
/// another trait's `Seq` or for a pack.
///
/// Implementations are found in every crate linked into the binary that runs
/// startup, the server binary of a fullstack app or the app itself in a
/// standalone build, the way Dioxus finds `#[server]` functions. Rust links a
/// crate only if something in the binary names it, so an implementation in a
/// crate that nothing names is missing from the collection; name that crate in
/// the binary's crate root with `use that_crate as _;`, under
/// `#[cfg(feature = "server")]` when it is a server-only dependency.
///
/// # Examples
///
/// ```rust,ignore
/// #[derive(Clone, haze::Pack)]
/// struct Recorder {
///     log: Arc<EntryLog>,
/// }
///
/// #[haze::register(order = 20)]
/// impl Interceptor for Recorder {
///     fn on_request(&self, request: &Request) {
///         self.log.record(request);
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn register(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let implementation = match syn::parse::<ItemImpl>(item.clone()) {
        Ok(implementation) => implementation,
        Err(error) => {
            let mut tokens = item;
            tokens.extend(TokenStream::from(error.to_compile_error()));
            return tokens;
        }
    };
    match Register::expand(attribute.into(), &implementation) {
        Ok(tokens) => tokens.into(),
        Err(error) => {
            let mut tokens = implementation.to_token_stream();
            tokens.extend(error.to_compile_error());
            tokens.into()
        }
    }
}

/// Turns a function into a resource that startup provides automatically,
/// whether `haze::serve` runs it on a server, `haze::launch` or
/// `haze::install` in a standalone app, or `haze::Resources::start` anywhere
/// else, such as a test.
///
/// The returned value is inserted into `haze::Resources` under its exact type;
/// a `Result<T, E>` inserts `T` and stops startup on `Err`. Functions may be
/// `async`. Each parameter is one of:
///
/// - `T`: fetched from `Resources`; the function waits until `T` exists.
/// - `Option<T>`: `None` if no one inserts or provides `T`; waits while
///   another function will still provide it.
/// - `Later<T>`: not waited for, filled by the end of startup; use it on one
///   side of a cycle, where two functions need each other, and for a
///   `haze::Seq<dyn Trait>` or a `#[derive(Pack)]` struct, which startup
///   installs only after these functions (as `T` they stop startup, and as
///   `Option<T>` they are always `None`).
///
/// These are the forms a `#[derive(Pack)]` field takes; `Res<T>` is the
/// server-function form and is rejected here. Parameters are taken by value,
/// as the clone the registry hands out, so a reference resource is
/// `&'static T`. `Option`, `Later` and `Res`, and `Result` in the return type,
/// are recognized by the last segment of the type's path, like summer's
/// wrappers: a type of your own with one of those names needs a type alias
/// here, and a return type written through an alias such as `AppResult<T>` is
/// not seen as a `Result`, so the whole alias type would be inserted; write
/// `Result<T, E>`. A function returns `T`, not `Option<T>`: a function that
/// can do without a `T` takes `Option<T>` instead.
///
/// Functions run in any order that satisfies these rules. When they wait on
/// each other in a cycle through an `Option<T>`, a function on that cycle runs
/// with `None`. A type inserted by hand in setup wins, and its function is
/// skipped. A missing type, a failing function, two functions for one type, or
/// a cycle of required parameters alone stops startup, before any function
/// runs when it can, with an error that names the functions and types involved.
///
/// Functions are found in every crate linked into the binary that runs
/// startup, the server binary of a fullstack app or the app itself in a
/// standalone build. Rust links a crate only if something in the binary names
/// it, and naming the returned type is not enough when that type is defined in
/// another crate; a function in a crate that nothing names never runs, and an
/// `Option<T>` of its type receives `None` without an error. Name such a crate
/// in the binary's crate root with `use that_crate as _;`, under
/// `#[cfg(feature = "server")]` when it is a server-only dependency.
///
/// # Examples
///
/// ```rust,ignore
/// use std::sync::Arc;
///
/// use anyhow::Result;
///
/// #[haze::resource]
/// async fn open_history() -> Result<History> {
///     History::open("data/history.redb").await
/// }
///
/// #[haze::resource]
/// fn recorder(history: History) -> Arc<Recorder> {
///     Arc::new(Recorder::new(history))
/// }
/// ```
#[proc_macro_attribute]
pub fn resource(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = match syn::parse::<ItemFn>(item.clone()) {
        Ok(function) => function,
        Err(error) => {
            let mut tokens = item;
            tokens.extend(TokenStream::from(error.to_compile_error()));
            return tokens;
        }
    };
    match Resource::expand(attribute.into(), &function) {
        Ok(tokens) => tokens.into(),
        Err(error) => {
            let mut tokens = function.to_token_stream();
            tokens.extend(error.to_compile_error());
            tokens.into()
        }
    }
}

/// Declares a server function whose handles are haze resources, for an app
/// that runs as a Dioxus fullstack app and, built with its own `standalone`
/// feature, in one process without a server.
///
/// The attribute takes the arguments of Dioxus's `#[server]`, positional or
/// `key = value` options such as `endpoint = "scores"`, followed by handle
/// pairs `name: Type`, which come last. Each handle is a resource of the
/// application's `haze::Resources`, written as one of:
///
/// - `T`: the value inserted as `T`, such as a `#[derive(Pack)]` struct; the
///   call fails with a `500` naming `T` when it is missing.
/// - `Option<T>`: `None` when no `T` was inserted.
/// - `Res<T>` or `Option<Res<T>>`: the same, still wrapped in `haze::Res`.
///
/// A handle is not a request extractor: a server function that needs the
/// request, a header or a cookie uses Dioxus's own `#[server]`, which only
/// runs on a server. The function is `async`, is not a method, and its error
/// type converts from `ServerFnError`, as Dioxus requires; `ServerFnError`,
/// `HttpError`, `anyhow::Error`, `StatusCode` and `dioxus::Result` all do.
/// `Option` and `Res` are recognized by the last segment of the type's path,
/// so a type of your own with one of those names needs a type alias here.
///
/// The attribute expands to two copies of the function, selected by the
/// application crate's `standalone` feature:
///
/// - Without it, Dioxus's `#[server]` with the same arguments, every `T`
///   handle passed on as `haze::Res<T>` and every `Option<T>` as
///   `Option<haze::Res<T>>`, and the body opening them first, so the body
///   sees the types it wrote. Dioxus extracts them from the `Resources` that
///   `haze::serve` attaches to each request, and everything else, such as the
///   client call, `#[middleware]`, the route and the encoding, is Dioxus's.
///   The function's crate depends on `dioxus-fullstack` and has
///   `dioxus_server` in scope, as for `#[server]`.
/// - With it, the function itself, run in process: its body first reads the
///   registry `haze::Resources::install_default` installed, which
///   `haze::launch` does, then fetches each handle from it, as one map
///   lookup and one clone. Nothing is sent over HTTP. A call before a registry
///   is installed fails with a `500` naming the function. `#[middleware]`
///   attributes are dropped, the route and options are not used, and a
///   function without handles is kept as it is.
///
/// `standalone` takes precedence: a build with both `server` and
/// `standalone` compiles the in-process copy, so Dioxus registers none of
/// these functions and the server answers their routes with `404`. Never
/// enable both in a build you run, which `--all-features` or a workspace's
/// feature unification can do.
///
/// Every crate that uses the attribute declares a `standalone` feature,
/// which enables `haze/standalone`, the way Dioxus asks for a `server`
/// feature; without it, the build fails with an `unexpected_cfgs` error whose
/// note shows the line to add. Put the attribute above any `#[middleware]`.
///
/// # Examples
///
/// ```rust,ignore
/// #[derive(Clone, haze::Pack)]
/// struct Scores {
///     store: Arc<Store>,
/// }
///
/// #[haze::server(scores: Scores, motto: Option<Motto>)]
/// async fn top(count: usize) -> Result<Vec<Score>, ServerFnError> {
///     let mut top = scores.store.top(count);
///     if let Some(motto) = motto {
///         top.push(motto.as_score());
///     }
///     Ok(top)
/// }
/// ```
///
/// In a fullstack build that is Dioxus's own server function, its handles
/// wrapped in haze's `Res`:
///
/// ```rust,ignore
/// #[server(scores: Res<Scores>, motto: Option<Res<Motto>>)]
/// async fn top(count: usize) -> Result<Vec<Score>, ServerFnError> {
///     let scores = scores.0;
///     let motto = motto.map(|res| res.0);
///     let mut top = scores.store.top(count);
///     if let Some(motto) = motto {
///         top.push(motto.as_score());
///     }
///     Ok(top)
/// }
/// ```
///
/// and in a `standalone` build an ordinary async function that reads haze's
/// process default `Resources`:
///
/// ```rust,ignore
/// async fn top(count: usize) -> Result<Vec<Score>, ServerFnError> {
///     let resources = Resources::get_default().ok_or_else(|| {
///         ServerFnError::new(
///             "top was called, but no haze Resources are installed; \
///              call Resources::install_default, which haze::launch does, before calling it",
///         )
///     })?;
///     let scores: Scores = resources.get::<Scores>().ok_or_else(|| {
///         ServerFnError::new(format!(
///             "{} was never inserted",
///             std::any::type_name::<Scores>(),
///         ))
///     })?;
///     let motto: Option<Motto> = resources.get::<Motto>();
///     let mut top = scores.store.top(count);
///     if let Some(motto) = motto {
///         top.push(motto.as_score());
///     }
///     Ok(top)
/// }
/// ```
#[proc_macro_attribute]
pub fn server(attribute: TokenStream, item: TokenStream) -> TokenStream {
    ServerFn::attribute(attribute, item, "server", Arguments::server)
}

/// Declares a `GET` server function at a route, whose handles are haze
/// resources; the route counterpart of [`macro@server`].
///
/// The attribute takes the arguments of Dioxus's `#[get]`: the route string,
/// whose path and query parameters are ordinary arguments of the function,
/// then `, ` and the handle pairs, `name: Type`, in the forms
/// [`macro@server`] lists. It expands the same way: Dioxus's `#[get]` without
/// the application's `standalone` feature, and the function run in process
/// with it, where the route is not used and its parameters stay plain
/// arguments.
///
/// # Examples
///
/// ```rust,ignore
/// #[haze::get("/api/history?page", history: History)]
/// async fn entries(page: u32) -> Result<Vec<Entry>, ServerFnError> {
///     Ok(history.page(page))
/// }
/// ```
#[proc_macro_attribute]
pub fn get(attribute: TokenStream, item: TokenStream) -> TokenStream {
    ServerFn::attribute(attribute, item, "get", Arguments::route)
}

/// Declares a `POST` server function at a route, whose handles are haze
/// resources, the way [`macro@get`] declares a `GET` one.
#[proc_macro_attribute]
pub fn post(attribute: TokenStream, item: TokenStream) -> TokenStream {
    ServerFn::attribute(attribute, item, "post", Arguments::route)
}

/// Declares a `PUT` server function at a route, whose handles are haze
/// resources, the way [`macro@get`] declares a `GET` one.
#[proc_macro_attribute]
pub fn put(attribute: TokenStream, item: TokenStream) -> TokenStream {
    ServerFn::attribute(attribute, item, "put", Arguments::route)
}

/// Declares a `DELETE` server function at a route, whose handles are haze
/// resources, the way [`macro@get`] declares a `GET` one.
#[proc_macro_attribute]
pub fn delete(attribute: TokenStream, item: TokenStream) -> TokenStream {
    ServerFn::attribute(attribute, item, "delete", Arguments::route)
}

/// Declares a `PATCH` server function at a route, whose handles are haze
/// resources, the way [`macro@get`] declares a `GET` one.
#[proc_macro_attribute]
pub fn patch(attribute: TokenStream, item: TokenStream) -> TokenStream {
    ServerFn::attribute(attribute, item, "patch", Arguments::route)
}
