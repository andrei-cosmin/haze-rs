#![cfg(feature = "standalone")]

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use dioxus::fullstack::ServerFnError;
use futures_util::FutureExt;
use haze::{Pack, Resources};
use tokio::runtime::{Handle, RuntimeFlavor};

#[derive(Clone, Debug, PartialEq)]
struct Season(&'static str);

#[derive(Clone, Pack)]
struct Board {
    season: Season,
    #[pack(func = Arc::default())]
    hits: Arc<AtomicU64>,
}

impl Board {
    fn hit(&self) -> u64 {
        self.hits.fetch_add(1, Ordering::Relaxed) + 1
    }
}

static INSTALLED: OnceLock<Resources> = OnceLock::new();

struct Installed;

impl Installed {
    fn resources() -> &'static Resources {
        INSTALLED.get_or_init(|| {
            haze::install(async |resources| {
                resources.insert(Season("spring"));
                resources.insert(Handle::current());
                Ok(())
            })
        })
    }
}

#[haze::server(board: Board)]
async fn season() -> Result<String, ServerFnError> {
    Ok(board.season.0.to_owned())
}

#[haze::server(board: Board)]
async fn bump() -> Result<u64, ServerFnError> {
    Ok(board.hit())
}

#[test]
fn install_returns_the_registry_it_made_the_process_default() {
    let resources = Installed::resources();
    let installed = Resources::get_default().unwrap();
    assert_eq!(installed.get::<Season>(), Some(Season("spring")));
    assert!(Arc::ptr_eq(
        &resources.get::<Board>().unwrap().hits,
        &installed.get::<Board>().unwrap().hits
    ));
}

#[test]
fn a_server_function_runs_on_a_plain_thread_without_a_runtime() {
    let resources = Installed::resources();
    let calls = thread::spawn(|| (season().now_or_never(), bump().now_or_never()))
        .join()
        .unwrap();
    assert_eq!(calls, (Some(Ok(String::from("spring"))), Some(Ok(1))));
    assert_eq!(
        resources
            .get::<Board>()
            .unwrap()
            .hits
            .load(Ordering::Relaxed),
        1
    );
}

#[test]
fn startup_runs_on_a_multi_thread_runtime_that_outlives_install() {
    let handle = Installed::resources().get::<Handle>().unwrap();
    assert_eq!(handle.runtime_flavor(), RuntimeFlavor::MultiThread);
    let (sender, receiver) = mpsc::channel();
    handle.spawn(async move { sender.send("ran").unwrap() });
    assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok("ran"));
}
