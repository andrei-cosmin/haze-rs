use std::{
    any::type_name,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::Result;
use haze::{Build, Later, Pack, Resources, Seq};

#[derive(Clone)]
struct Motto(&'static str);

#[derive(Clone, Default)]
struct Forge(Arc<AtomicU64>);

trait Badge: Send + Sync {
    fn label(&self) -> String;

    fn count(&self) -> u64;
}

struct Gold(Forge);

impl Build for Gold {
    fn build(resources: &Resources) -> Result<Self> {
        let forge = resources.try_get::<Forge>()?;
        forge.0.fetch_add(1, Ordering::Relaxed);
        Ok(Self(forge))
    }
}

#[haze::register(order = 1)]
impl Badge for Gold {
    fn label(&self) -> String {
        String::from("gold")
    }

    fn count(&self) -> u64 {
        self.0.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Pack)]
struct Silver {
    #[pack(func = Arc::default())]
    hits: Arc<AtomicU64>,
}

#[haze::register(order = 2)]
impl Badge for Silver {
    fn label(&self) -> String {
        String::from("silver")
    }

    fn count(&self) -> u64 {
        self.hits.fetch_add(1, Ordering::Relaxed) + 1
    }
}

#[derive(Clone, Pack)]
struct Scores {
    name: String,
    counter: Arc<u64>,
    tag: &'static str,
    motto: Option<Motto>,
    badges: Seq<dyn Badge>,
}

#[derive(Clone, Pack)]
struct Leaderboard {
    scores: Scores,
    keeper: Keeper,
}

#[derive(Clone, Pack)]
struct Keeper {
    board: Later<Leaderboard>,
}

#[derive(Clone, Pack)]
struct Tally {
    #[pack(func = Self::label(&scores))]
    label: String,
    #[pack(func = Self::banner(&label))]
    banner: String,
    #[pack(func = Arc::default())]
    hits: Arc<AtomicU64>,
    scores: Scores,
}

impl Tally {
    fn label(scores: &Scores) -> String {
        format!("{} scored", scores.name)
    }

    fn banner(label: &str) -> String {
        label.to_uppercase()
    }
}

#[derive(Clone)]
struct Trophy {
    board: Later<Leaderboard>,
    early: Option<Scores>,
}

#[haze::resource]
fn trophy(board: Later<Leaderboard>, scores: Option<Scores>) -> Trophy {
    Trophy {
        board,
        early: scores,
    }
}

struct Setup;

impl Setup {
    #[allow(clippy::unnecessary_wraps)]
    fn complete(resources: &mut Resources, counter: Arc<u64>) -> Result<()> {
        resources.insert(String::from("ana"));
        resources.insert(counter);
        resources.insert("season one");
        resources.insert(Forge::default());
        Ok(())
    }
}

#[tokio::test]
async fn start_builds_a_pack_from_its_fields_and_inserts_it() {
    let counter = Arc::new(3_u64);
    let shared = Arc::clone(&counter);
    let resources = Resources::start(async move |resources| Setup::complete(resources, shared))
        .await
        .unwrap();
    let scores = resources.get::<Scores>().unwrap();
    assert_eq!(scores.name, "ana");
    assert!(Arc::ptr_eq(&scores.counter, &counter));
    assert_eq!(scores.tag, "season one");
    assert!(scores.motto.is_none());
    assert_eq!(scores.badges[0].label(), "gold");
}

#[tokio::test]
async fn an_optional_field_is_filled_when_its_type_is_inserted() {
    let resources = Resources::start(async |resources| {
        resources.insert(Motto("keep going"));
        Setup::complete(resources, Arc::new(0))
    })
    .await
    .unwrap();
    let scores = resources.get::<Scores>().unwrap();
    assert_eq!(scores.motto.map(|motto| motto.0), Some("keep going"));
}

#[tokio::test]
async fn a_pack_holding_another_is_built_after_it() {
    let counter = Arc::new(5_u64);
    let shared = Arc::clone(&counter);
    let resources = Resources::start(async move |resources| Setup::complete(resources, shared))
        .await
        .unwrap();
    let board = resources.get::<Leaderboard>().unwrap();
    assert!(Arc::ptr_eq(&board.scores.counter, &counter));
}

#[tokio::test]
async fn a_later_field_closes_a_cycle_between_packs() {
    let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
        .await
        .unwrap();
    let board = resources.get::<Leaderboard>().unwrap();
    assert_eq!(board.keeper.board.get().unwrap().scores.name, "ana");
}

#[tokio::test]
async fn a_registered_pack_is_the_one_instance_startup_built() {
    let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
        .await
        .unwrap();
    let badges = resources.get::<Seq<dyn Badge>>().unwrap();
    assert_eq!(badges[1].label(), "silver");
    badges[1].count();
    badges[1].count();
    let silver = resources.get::<Silver>().unwrap();
    assert_eq!(silver.hits.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn a_build_implementation_runs_once_even_when_the_collector_waits_a_round() {
    assert!(type_name::<Seq<dyn Badge>>() < type_name::<Silver>());
    let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
        .await
        .unwrap();
    let badges = resources.get::<Seq<dyn Badge>>().unwrap();
    assert_eq!(badges[0].count(), 1);
    let forge = resources.get::<Forge>().unwrap();
    assert_eq!(forge.0.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn func_fields_run_last_in_declaration_order_and_see_every_other_field() {
    let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
        .await
        .unwrap();
    let tally = resources.get::<Tally>().unwrap();
    assert_eq!(tally.scores.name, "ana");
    assert_eq!(tally.label, "ana scored");
    assert_eq!(tally.banner, "ANA SCORED");
    assert_eq!(tally.hits.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_pack_inserted_in_setup_is_kept() {
    let resources = Resources::start(async |resources| {
        resources.insert(Scores {
            name: String::from("by hand"),
            counter: Arc::new(0),
            tag: "by hand",
            motto: None,
            badges: Seq::from(Vec::new()),
        });
        resources.insert(Forge::default());
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        resources.get::<Leaderboard>().unwrap().scores.name,
        "by hand"
    );
}

#[tokio::test]
async fn a_resource_function_reaches_a_pack_only_through_later() {
    let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
        .await
        .unwrap();
    let trophy = resources.get::<Trophy>().unwrap();
    assert!(trophy.early.is_none());
    assert_eq!(trophy.board.get().unwrap().scores.name, "ana");
}

#[tokio::test]
async fn start_stops_naming_every_item_left_and_the_type_it_misses() {
    let Err(error) = Resources::start(async |resources| {
        resources.insert(String::from("ana"));
        Ok(())
    })
    .await
    else {
        panic!("start succeeded although Scores cannot be built");
    };
    assert_eq!(
        error.to_string(),
        "collecting pack::Gold as dyn pack::Badge: pack::Forge was never inserted; pack::Leaderboard needs pack::Scores, which was never inserted; pack::Scores needs alloc::sync::Arc<u64>, which was never inserted; pack::Tally needs pack::Scores, which was never inserted"
    );
}

#[cfg(feature = "server")]
mod request {
    use std::{
        future::{Ready, ready},
        sync::Arc,
    };

    use dioxus::{
        fullstack::{
            FullstackContext,
            axum_core::extract::{FromRequestParts, OptionalFromRequestParts},
            http::{Request, StatusCode, request::Parts},
        },
        server::axum::{
            Extension, Router,
            body::{Body, to_bytes},
            routing::get,
        },
    };
    use haze::{Res, Resources};
    use tower::ServiceExt;

    use super::{Leaderboard, Scores, Setup, Tally};

    struct Incoming;

    impl Incoming {
        fn with(resources: Resources) -> Parts {
            let mut parts = Self::bare();
            parts.extensions.insert(resources);
            parts
        }

        fn bare() -> Parts {
            Request::new(()).into_parts().0
        }
    }

    struct Handler;

    impl Handler {
        fn board(Res(board): Res<Leaderboard>) -> Ready<String> {
            ready(board.scores.name)
        }
    }

    #[tokio::test]
    async fn a_server_function_receives_the_built_pack_through_res() {
        let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
            .await
            .unwrap();
        let mut parts = Incoming::with(resources);
        let Res(board) =
            <Res<Leaderboard> as FromRequestParts<()>>::from_request_parts(&mut parts, &())
                .await
                .unwrap();
        assert_eq!(board.scores.name, "ana");
    }

    #[tokio::test]
    async fn a_server_function_receives_the_built_pack_through_an_optional_res() {
        let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
            .await
            .unwrap();
        let mut parts = Incoming::with(resources);
        let present =
            <Res<Leaderboard> as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &())
                .await
                .unwrap();
        assert_eq!(
            present.map(|Res(board)| board.scores.name),
            Some(String::from("ana"))
        );
    }

    #[tokio::test]
    async fn res_hands_out_the_instance_startup_built() {
        let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
            .await
            .unwrap();
        let built = resources.get::<Tally>().unwrap();
        let mut parts = Incoming::with(resources);
        let Res(tally) = <Res<Tally> as FromRequestParts<()>>::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&tally.hits, &built.hits));
    }

    #[tokio::test]
    async fn a_pack_is_extracted_through_res_the_way_dioxus_extracts_server_function_arguments() {
        let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
            .await
            .unwrap();
        let (Res(board), scores) = FullstackContext::new(Incoming::with(resources))
            .scope(FullstackContext::extract::<
                (Res<Leaderboard>, Option<Res<Scores>>),
                _,
            >())
            .await
            .unwrap();
        assert_eq!(board.scores.name, "ana");
        assert_eq!(
            scores.map(|Res(scores)| scores.name),
            Some(String::from("ana"))
        );
    }

    #[tokio::test]
    async fn a_pack_missing_from_the_registry_is_none_when_optional_and_a_500_when_not() {
        let mut parts = Incoming::with(Resources::new());
        let absent =
            <Res<Leaderboard> as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &())
                .await
                .unwrap();
        assert!(absent.is_none());
        let Err(error) =
            <Res<Leaderboard> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
        else {
            panic!("an absent pack was extracted");
        };
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            error.message.as_deref(),
            Some("pack::Leaderboard was never inserted")
        );
    }

    #[tokio::test]
    async fn a_request_without_a_registry_is_a_500_even_for_an_optional_pack() {
        let mut parts = Incoming::bare();
        let Err(error) =
            <Res<Leaderboard> as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &())
                .await
        else {
            panic!("an optional pack was extracted without a registry");
        };
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            error.message.as_deref(),
            Some(
                "pack::Leaderboard was requested, but no haze Resources are attached to this request; start the router with haze::serve or layer Extension(resources), and call the server function inside a request"
            )
        );
        let Err(error) =
            <Res<Leaderboard> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
        else {
            panic!("a pack was extracted without a registry");
        };
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn an_axum_handler_receives_the_pack_through_res_over_a_request() {
        let resources = Resources::start(async |resources| Setup::complete(resources, Arc::new(0)))
            .await
            .unwrap();
        let router = Router::new()
            .route("/board", get(Handler::board))
            .layer(Extension(resources));
        let response = router
            .oneshot(Request::get("/board").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body, "ana");
    }
}
