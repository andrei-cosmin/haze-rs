//! Starting resources and launching a Dioxus app in one process, without a
//! server.

#[cfg(not(target_family = "wasm"))]
use std::sync::OnceLock;

use anyhow::Result;
use dioxus::core::Element;
#[cfg(not(target_family = "wasm"))]
use tokio::runtime::{Builder, Runtime};

use crate::Resources;

/// The tokio runtime [`install`] starts the registry on and [`launch`] runs
/// the app in, kept for the rest of the process.
#[cfg(not(target_family = "wasm"))]
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Why a second registry cannot become the process default.
#[cfg(target_family = "wasm")]
const ALREADY_INSTALLED: &str = "default haze Resources already set elsewhere";

/// Starts the application's [`Resources`] and makes them the process default,
/// for an app that runs without a server.
///
/// Runs [`Resources::start`] with `setup` on a multi-thread tokio runtime that
/// haze keeps for the rest of the process, so tasks spawned during startup
/// keep running, makes the result the process default, where the `standalone`
/// copy of every [`#[server]`](macro@crate::server) function reads it, and
/// returns a clone. Startup runs once per process: a second call returns the
/// registry the first one built without running `setup` again, the way
/// [`serve`](fn@crate::serve) reuses its startup after a server hot-patch.
/// [`launch`] calls it before launching Dioxus; call it directly to launch
/// Dioxus with a configuration of your own, or in a program without a user
/// interface.
///
/// # Panics
///
/// Panics with the error and its causes when startup fails, the way a
/// fullstack server started by [`serve`](fn@crate::serve) stops.
///
/// Panics when called within an asynchronous execution context, such as under
/// `#[tokio::main]`, as tokio's `block_on` does. There, await
/// [`Resources::start`] and pass its registry to
/// [`Resources::install_default`], which report a failed startup as an error
/// instead of panicking.
///
/// # Examples
///
/// ```rust,ignore
/// fn main() {
///     let resources = haze::install(async |resources| {
///         resources.insert(Clicks::default());
///         Ok(())
///     });
///     assert!(resources.contains::<Clicks>());
/// }
/// ```
#[cfg(not(target_family = "wasm"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub fn install<S>(setup: S) -> Resources
where
    S: AsyncFnOnce(&mut Resources) -> Result<()>,
{
    let runtime = RUNTIME.get_or_init(|| Builder::new_multi_thread().enable_all().build().unwrap());
    Resources::get_or_install(|| runtime.block_on(Resources::start(setup)).unwrap()).clone()
}

/// Starts the application's [`Resources`], makes them the process default and
/// launches the Dioxus app `app`, for an app that runs without a server.
///
/// Every [`#[server]`](macro@crate::server) function built with the
/// application's own `standalone` feature then runs in process, its handles
/// read from the installed registry, so the app needs no server at all.
///
/// On native targets it calls [`install`], enters the runtime `install`
/// keeps and calls `dioxus::launch(app)`: Dioxus's desktop renderer runs on
/// that runtime, and its native renderer starts one of its own inside it. It
/// returns when Dioxus does, which on desktop is never. On `wasm32` it runs
/// startup in a browser task with `wasm_bindgen_futures::spawn_local` and
/// returns at once; Dioxus is launched when startup finishes, so the page
/// stays empty until then.
///
/// The renderer is chosen by Dioxus: enable `desktop`, `mobile`, `native` or
/// `web` on the application's own `dioxus` dependency, or `dioxus::launch`
/// panics. Leave `dioxus`'s `fullstack` feature off in a standalone web
/// build, since it turns on hydration, which reads a page rendered by a
/// server.
///
/// # Panics
///
/// Panics as [`install`] does. On `wasm32` the panic happens inside the
/// startup task and is reported in the browser console.
///
/// # Examples
///
/// ```rust,ignore
/// fn main() {
///     haze::launch(
///         async |resources| {
///             resources.insert(Clicks::default());
///             Ok(())
///         },
///         App,
///     );
/// }
/// ```
///
/// To configure the launch, such as the desktop window, install the registry
/// with [`install`] and launch with Dioxus's `LaunchBuilder`:
///
/// ```rust,ignore
/// fn main() {
///     let _resources = haze::install(setup);
///     dioxus::LaunchBuilder::new()
///         .with_cfg(desktop! {
///             Config::new().with_window(WindowBuilder::new().with_title("Clicks"))
///         })
///         .launch(App);
/// }
/// ```
///
/// The desktop renderer then starts a runtime of its own, and tasks spawned
/// during startup keep running on haze's. On `wasm32`, which has no
/// `install`, call [`Resources::start`] and [`Resources::install_default`]
/// inside `wasm_bindgen_futures::spawn_local`, then the builder.
pub fn launch<S>(setup: S, app: fn() -> Element)
where
    S: AsyncFnOnce(&mut Resources) -> Result<()> + 'static,
{
    #[cfg(not(target_family = "wasm"))]
    {
        let _resources = install(setup);
        let Some(runtime) = RUNTIME.get() else {
            unreachable!("haze::install starts the runtime");
        };
        let _guard = runtime.enter();
        dioxus::launch(app);
    }

    #[cfg(target_family = "wasm")]
    {
        wasm_bindgen_futures::spawn_local(async move {
            Resources::start(setup)
                .await
                .unwrap()
                .install_default()
                .expect(ALREADY_INSTALLED);
            dioxus::launch(app);
        });
    }
}
