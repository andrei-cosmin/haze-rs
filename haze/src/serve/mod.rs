//! Starting the Dioxus server with resources, and a client-rendered router.

mod client_rendered;
mod startup;

pub use client_rendered::client_rendered;

use std::rc::Rc;

use anyhow::Result;
use dioxus::server::axum::{Extension, Router};

use crate::Resources;
use startup::Startup;

/// Runs a Dioxus fullstack server whose server functions can inject resources.
///
/// On first start, haze builds a [`Resources`] registry with
/// [`Resources::start`], in four steps:
///
/// 1. `setup` fills it by hand.
/// 2. [`Resources::provide`] runs every [`#[resource]`](macro@crate::resource)
///    function whose type is still missing.
/// 3. Every [`#[register]`](macro@crate::register)ed trait whose `Seq` is still
///    missing is collected, and every [`#[derive(Pack)]`](derive@crate::Pack)
///    struct whose type is still missing is built and inserted, each waiting
///    for what it needs.
/// 4. [`Resources::finish`] fills every [`Later`](crate::Later) handed out.
///
/// `router` builds the app's router the Dioxus way, in any mode Dioxus
/// supports; haze attaches the registry to it and hands it to Dioxus's
/// [`serve`](dioxus::server::serve), which owns the address, hot reload and
/// everything else.
///
/// During `dx serve --hot-patch`, Dioxus rebuilds the router after each server
/// hot-patch by calling `router` again. The registry from the first start is
/// reused, so exclusive resources such as a database file are never opened twice.
///
/// - The four steps above run only on first start. Changes to `setup`, resource
///   functions, registered implementations or packs need a restart.
/// - Subsecond does not hot-reload structs, and the registry keeps the values
///   built on first start.
/// - A patch that adds, removes or renames a field of a type the registry holds
///   gives that type a new `TypeId`. Nothing is found under it, and `Res<T>`
///   rejects every call with a `500` naming it.
/// - A patch that changes a field's type under the same name makes the patched
///   code read the old value with the new layout, and the program crashes.
///
/// Restart `dx serve` after changing a type the registry holds.
///
/// If `setup`, a resource function, a registered implementation's build or a
/// pack fails, or a resource function, registered implementation, pack or
/// [`Later`](crate::Later) needs a type that was never inserted, the server
/// does not start and the error is reported with its causes. A
/// [`Res<T>`](crate::Res) argument of a server function is not known at
/// startup: a missing `T` rejects that call with a `500` naming `T`, the way a
/// missing axum `Extension` rejects the request.
///
/// # Examples
///
/// Server-side rendering, the Dioxus default:
///
/// ```rust,ignore
/// fn main() {
///     #[cfg(not(feature = "server"))]
///     dioxus::launch(App);
///
///     #[cfg(feature = "server")]
///     haze::serve(
///         async |resources| {
///             resources.insert(Arc::new(Storage::open("data.redb")?));
///             Ok(())
///         },
///         || dioxus::server::router(App),
///     );
/// }
/// ```
///
/// Any other Dioxus router works the same way, for example client-rendered
/// pages built from `register_server_functions` and `serve_static_assets`, or
/// streaming SSR through `serve_dioxus_application` with a `ServeConfig`.
pub fn serve<S, R>(setup: S, mut router: R) -> !
where
    S: AsyncFnOnce(&mut Resources) -> Result<()> + 'static,
    R: FnMut() -> Router,
{
    let startup = Rc::new(Startup::new(setup));
    dioxus::server::serve(move || {
        let startup = Rc::clone(&startup);
        let router = router();
        async move { Ok(router.layer(Extension(startup.resources().await?))) }
    })
}
