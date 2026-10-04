use anyhow::Result;
use haze::{Build, Resources, Seq};

#[haze::collect(1)]
trait Missing: Send + Sync {}

#[haze::collect(1)]
trait Crowded: Send + Sync {
    fn name(&self) -> &'static str;
}

struct First;

impl Build for First {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = 1)]
impl Crowded for First {
    fn name(&self) -> &'static str {
        "first"
    }
}

struct Second;

impl Build for Second {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = 2)]
impl Crowded for Second {
    fn name(&self) -> &'static str {
        "second"
    }
}

struct StandIn;

impl Crowded for StandIn {
    fn name(&self) -> &'static str {
        "stand-in"
    }
}

#[test]
fn a_count_of_one_fails_with_no_registration_naming_the_trait_and_both_counts() {
    let error = Resources::new().collect::<dyn Missing>().unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "dyn collect_count::Missing is declared with #[haze::collect(1)], so it needs exactly 1 registered implementation, but 0 are registered"
    );
}

#[test]
fn a_count_of_one_fails_with_two_registrations_naming_the_trait_and_both_counts() {
    let error = Resources::new().collect::<dyn Crowded>().unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "dyn collect_count::Crowded is declared with #[haze::collect(1)], so it needs exactly 1 registered implementation, but 2 are registered"
    );
}

#[tokio::test]
async fn startup_fails_naming_every_trait_whose_count_differs() {
    let Err(error) = Resources::start(async |_| Ok(())).await else {
        panic!("start succeeded although the counts differ");
    };
    assert_eq!(
        format!("{error:#}"),
        "dyn collect_count::Crowded is declared with #[haze::collect(1)], so it needs exactly 1 registered implementation, but 2 are registered; dyn collect_count::Missing is declared with #[haze::collect(1)], so it needs exactly 1 registered implementation, but 0 are registered"
    );
}

#[tokio::test]
async fn a_seq_inserted_in_setup_is_kept_without_the_count_check() {
    let resources = Resources::start(async |resources| {
        let missing: Vec<Box<dyn Missing>> = Vec::new();
        resources.insert(Seq::from(missing));
        let crowded: Vec<Box<dyn Crowded>> = vec![Box::new(StandIn)];
        resources.insert(Seq::from(crowded));
        Ok(())
    })
    .await
    .unwrap();
    assert!(resources.get::<Seq<dyn Missing>>().unwrap().is_empty());
    assert_eq!(
        resources.get::<Seq<dyn Crowded>>().unwrap()[0].name(),
        "stand-in"
    );
}
