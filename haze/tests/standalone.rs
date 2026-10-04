#![cfg(feature = "standalone")]

use std::{
    cell::RefCell,
    hint::black_box,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use dioxus::{
    core::NoOpMutations,
    fullstack::{HttpError, ServerFnError, StatusCode, Streaming},
    prelude::*,
};
use haze::{Pack, Res, Resources};
use tokio::sync::{OnceCell, watch::Sender};
use tokio_stream::wrappers::WatchStream;

#[derive(Clone)]
struct Season(&'static str);

#[derive(Clone)]
struct Absent;

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

#[derive(Clone, Pack)]
struct Ticker {
    #[pack(func = Sender::new(0))]
    sender: Sender<u32>,
}

#[derive(Clone, Default)]
struct Log {
    items: Rc<RefCell<Vec<u32>>>,
}

impl PartialEq for Log {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }
}

impl Log {
    fn items(&self) -> Vec<u32> {
        self.items.borrow().clone()
    }

    async fn drive(dom: &mut VirtualDom, until: impl Fn() -> bool) {
        dom.rebuild_in_place();
        for _ in 0..400 {
            if until() {
                return;
            }
            tokio::select! {
                () = dom.wait_for_work() => {}
                () = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            dom.render_immediate(&mut NoOpMutations);
        }
        panic!("the hook never reached the expected state");
    }
}

static BEFORE_INSTALL: OnceCell<ServerFnError> = OnceCell::const_new();

struct Installed;

impl Installed {
    async fn before_install() -> &'static ServerFnError {
        BEFORE_INSTALL
            .get_or_init(|| async {
                let error = season().await.unwrap_err();
                Resources::start(async |resources| {
                    resources.insert(Season("spring"));
                    Ok(())
                })
                .await
                .unwrap()
                .install_default()
                .unwrap();
                error
            })
            .await
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

#[haze::server(board: Option<Board>, absent: Option<Absent>)]
async fn optional() -> Result<(bool, bool), ServerFnError> {
    Ok((board.is_some(), absent.is_some()))
}

#[haze::server(board: Res<Board>, season: Option<Res<Season>>, absent: Option<Res<Absent>>)]
async fn wrapped() -> Result<(&'static str, bool, bool), ServerFnError> {
    let Res(board) = board;
    Ok((board.season.0, season.is_some(), absent.is_some()))
}

#[haze::server(absent: Absent)]
async fn missing() -> Result<(), ServerFnError> {
    let Absent = absent;
    Ok(())
}

#[haze::server(absent: Absent)]
async fn missing_as_http() -> Result<(), HttpError> {
    let Absent = absent;
    Ok(())
}

#[haze::get("/x?page", board: Board)]
async fn paged(page: u32) -> Result<String, ServerFnError> {
    Ok(format!("{} page {page}", board.season.0))
}

#[haze::post("/hit", board: Board)]
async fn hit(times: u64) -> Result<u64, ServerFnError> {
    let mut last = 0;
    for _ in 0..times {
        last = board.hit();
    }
    Ok(last)
}

#[haze::server(ticker: Ticker)]
async fn ticks() -> Result<Streaming<u32>, ServerFnError> {
    Ok(Streaming::new(WatchStream::new(ticker.sender.subscribe())))
}

#[component]
fn Ticks(log: Log) -> Element {
    let items = log.items.clone();
    haze::use_streaming(ticks, move |tick| items.borrow_mut().push(tick));
    rsx! {}
}

#[tokio::test]
async fn calling_before_install_names_the_function_and_the_fix() {
    assert_eq!(
        Installed::before_install().await,
        &ServerFnError::ServerError {
            message: String::from(
                "season was called, but no haze Resources are installed; call Resources::install_default, which haze::launch does, before calling it"
            ),
            code: 500,
            details: None,
        }
    );
}

#[tokio::test]
async fn a_pack_handle_is_fetched_from_the_installed_default() {
    Installed::before_install().await;
    assert_eq!(season().await.unwrap(), "spring");
}

#[tokio::test]
async fn every_call_receives_the_one_pack_startup_built() {
    Installed::before_install().await;
    let first = bump().await.unwrap();
    let second = bump().await.unwrap();
    assert!(second > first);
    let installed = Resources::get_default().unwrap().get::<Board>().unwrap();
    assert!(installed.hits.load(Ordering::Relaxed) >= second);
}

#[tokio::test]
async fn an_optional_handle_is_none_only_when_its_type_is_missing() {
    Installed::before_install().await;
    assert_eq!(optional().await.unwrap(), (true, false));
}

#[tokio::test]
async fn a_res_handle_is_wrapped_in_res() {
    Installed::before_install().await;
    assert_eq!(wrapped().await.unwrap(), ("spring", true, false));
}

#[tokio::test]
async fn a_missing_handle_is_a_500_naming_its_type() {
    Installed::before_install().await;
    assert_eq!(
        missing().await.unwrap_err(),
        ServerFnError::ServerError {
            message: String::from("standalone::Absent was never inserted"),
            code: 500,
            details: None,
        }
    );
    let error = missing_as_http().await.unwrap_err();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        error.message.as_deref(),
        Some(
            "error running server function: standalone::Absent was never inserted (details: None)"
        )
    );
}

#[tokio::test]
async fn a_route_query_parameter_stays_a_plain_argument() {
    Installed::before_install().await;
    assert_eq!(paged(3).await.unwrap(), "spring page 3");
}

#[tokio::test]
async fn a_post_route_takes_its_arguments_and_its_handle() {
    Installed::before_install().await;
    let before = bump().await.unwrap();
    assert!(hit(2).await.unwrap() >= before + 2);
}

#[tokio::test(start_paused = true)]
async fn use_streaming_receives_every_tick_a_pack_sends_unencoded_through_a_server_function() {
    Installed::before_install().await;
    let _unencoded: Streaming<u32, ()> = ticks().await.unwrap();
    let ticker = Resources::get_default().unwrap().get::<Ticker>().unwrap();
    tokio::spawn(async move {
        for tick in 1..=3 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            ticker.sender.send(tick).unwrap();
        }
    });
    let log = Log::default();
    let mut dom = VirtualDom::new_with_props(Ticks, TicksProps { log: log.clone() });
    Log::drive(&mut dom, || log.items().len() >= 4).await;
    assert_eq!(log.items(), [0, 1, 2, 3]);
}

#[tokio::test]
#[ignore = "a measurement; run with --release -- --ignored --nocapture"]
async fn a_standalone_call_costs_one_lookup_over_the_method() {
    const CALLS: u32 = 10_000_000;
    Installed::before_install().await;
    let board = Resources::get_default().unwrap().get::<Board>().unwrap();
    let started = Instant::now();
    for _ in 0..CALLS {
        black_box(board.hit());
    }
    let direct = started.elapsed();
    let started = Instant::now();
    for _ in 0..CALLS {
        black_box(bump().await.unwrap());
    }
    let standalone = started.elapsed();
    println!(
        "direct method: {:.2} ns per call; #[haze::server] with one handle: {:.2} ns per call",
        direct.as_secs_f64() * 1e9 / f64::from(CALLS),
        standalone.as_secs_f64() * 1e9 / f64::from(CALLS),
    );
}
