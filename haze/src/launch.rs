//! Starting resources and launching a Dioxus app in one process, without a
//! server.

#[cfg(not(target_family = "wasm"))]
use std::sync::OnceLock;

use anyhow::Result;
use dioxus::LaunchBuilder;
use dioxus::core::Element;
#[cfg(not(target_family = "wasm"))]
use tokio::runtime::{Builder, Handle, Runtime};

use crate::Resources;

/// The tokio runtime [`install`] and [`launch`] fall back to when the calling
/// thread has not entered one, kept for the rest of the process.
#[cfg(not(target_family = "wasm"))]
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[cfg(not(target_family = "wasm"))]
struct Standalone;

#[cfg(not(target_family = "wasm"))]
impl Standalone {
    /// The runtime the calling thread has entered, or [`RUNTIME`], built on first
    /// use.
    fn handle() -> Handle {
        Handle::try_current().unwrap_or_else(|_| {
            RUNTIME
                .get_or_init(|| Builder::new_multi_thread().enable_all().build().unwrap())
                .handle()
                .clone()
        })
    }

    fn install<S>(setup: S) -> &'static Resources
    where
        S: AsyncFnOnce(&mut Resources) -> Result<()>,
    {
        Resources::get_or_install(|| Self::handle().block_on(Resources::start(setup)).unwrap())
    }
}

/// Why a second registry cannot become the process default.
#[cfg(target_family = "wasm")]
const ALREADY_INSTALLED: &str = "default haze Resources already set elsewhere";

/// Starts the application's [`Resources`] and makes them the process default,
/// for an app that runs without a server.
///
/// Runs [`Resources::start`] with `setup`, makes the result the process
/// default and returns a clone. The `standalone` copy of every
/// [`#[server]`](macro@crate::server) function reads that default.
///
/// - Startup runs on the tokio runtime the calling thread has entered, or,
///   when it has entered none, on a multi-thread runtime that haze builds
///   and keeps for the rest of the process. Either way tasks spawned during
///   startup keep running. Build and enter a runtime first to choose its
///   size, or to share one with everything launched after `install`.
/// - Startup runs once per process. A second call returns the same registry
///   without running `setup` again, as [`serve`](fn@crate::serve) does after
///   a server hot-patch.
/// - [`launch`] calls this before launching Dioxus. Call it directly in a
///   program without a user interface.
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
    Standalone::install(setup).clone()
}

/// Starts the application's [`Resources`], makes them the process default and
/// launches the Dioxus app `app` with `builder`, for an app that runs without
/// a server.
///
/// Every [`#[server]`](macro@crate::server) function built with the
/// application's own `standalone` feature then runs in process, its handles
/// read from the installed registry, so the app needs no server at all.
///
/// `builder` is Dioxus's `LaunchBuilder`, so the launch is configured as it
/// is with Dioxus alone: `LaunchBuilder::new()` for the defaults, which is
/// what `dioxus::launch` uses, or `LaunchBuilder::desktop().with_cfg(..)` for
/// a window of your own.
///
/// On native targets it calls [`install`], enters the runtime `install`
/// used and calls `builder.launch(app)`: Dioxus's desktop renderer runs on
/// that runtime, and its native renderer starts one of its own inside it. It
/// returns when Dioxus does, which on desktop is never. On `wasm32` it runs
/// startup in a browser task with `wasm_bindgen_futures::spawn_local` and
/// returns at once; Dioxus is launched when startup finishes, so the page
/// stays empty until then.
///
/// The renderer is chosen by Dioxus: enable `desktop`, `mobile`, `native` or
/// `web` on the application's own `dioxus` dependency, or the launch panics.
/// Leave `dioxus`'s `fullstack` feature off in a standalone web build, since
/// it turns on hydration, which reads a page rendered by a server.
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
///         LaunchBuilder::new(),
///         App,
///     );
/// }
/// ```
///
/// With a desktop window of your own:
///
/// ```rust,ignore
/// fn main() {
///     haze::launch(
///         setup,
///         LaunchBuilder::desktop().with_cfg(
///             Config::new().with_window(WindowBuilder::new().with_title("Clicks")),
///         ),
///         App,
///     );
/// }
/// ```
pub fn launch<S>(setup: S, builder: LaunchBuilder, app: fn() -> Element)
where
    S: AsyncFnOnce(&mut Resources) -> Result<()> + 'static,
{
    #[cfg(not(target_family = "wasm"))]
    {
        Standalone::install(setup);
        let _guard = Standalone::handle().enter();
        builder.launch(app);
    }

    #[cfg(target_family = "wasm")]
    {
        wasm_bindgen_futures::spawn_local(async move {
            Resources::start(setup)
                .await
                .unwrap()
                .install_default()
                .expect(ALREADY_INSTALLED);
            builder.launch(app);
        });
    }
}
