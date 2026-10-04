use anyhow::Result;
use haze::{Build, Resources, Seq};

mod early {
    use anyhow::Result;
    use haze::{Build, Resources};

    use super::Pair;

    pub(crate) struct Left;

    impl Build for Left {
        fn build(_resources: &Resources) -> Result<Self> {
            Ok(Self)
        }
    }

    #[haze::register(order = 1)]
    impl Pair for Left {
        fn side(&self) -> &'static str {
            "left"
        }
    }
}

#[haze::collect]
trait Unclaimed: Send + Sync {}

#[haze::collect(1)]
trait Single: Send + Sync {
    fn name(&self) -> &'static str;
}

#[haze::collect(2)]
trait Pair: Send + Sync {
    fn side(&self) -> &'static str;
}

#[haze::collect]
trait Shared: Send + Sync {
    fn name(&self) -> &'static str;
}

struct Only;

impl Build for Only {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = 1)]
impl Single for Only {
    fn name(&self) -> &'static str {
        "only"
    }
}

#[haze::register(order = 1)]
impl Shared for Only {
    fn name(&self) -> &'static str {
        "shared"
    }
}

struct Right;

impl Build for Right {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = 2)]
impl Pair for Right {
    fn side(&self) -> &'static str {
        "right"
    }
}

#[tokio::test]
async fn a_declared_trait_with_no_implementation_is_collected_empty_at_startup() {
    let resources = Resources::start(async |_| Ok(())).await.unwrap();
    assert!(resources.get::<Seq<dyn Unclaimed>>().unwrap().is_empty());
}

#[tokio::test]
async fn a_count_of_one_passes_with_one_registration() {
    let resources = Resources::start(async |_| Ok(())).await.unwrap();
    let single = resources.get::<Seq<dyn Single>>().unwrap();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].name(), "only");
}

#[tokio::test]
async fn a_count_of_two_passes_with_two_registrations_from_anywhere_in_the_crate() {
    let resources = Resources::start(async |_| Ok(())).await.unwrap();
    let pair = resources.get::<Seq<dyn Pair>>().unwrap();
    let mut sides = Vec::new();
    for item in pair {
        sides.push(item.side());
    }
    assert_eq!(sides, ["left", "right"]);
}

#[tokio::test]
async fn a_declared_trait_coexists_with_its_registrations() {
    let resources = Resources::start(async |_| Ok(())).await.unwrap();
    assert_eq!(
        resources.get::<Seq<dyn Shared>>().unwrap()[0].name(),
        "shared"
    );
}

#[test]
fn collecting_a_declared_trait_by_hand_checks_the_count_and_passes() {
    let mut resources = Resources::new();
    resources.collect::<dyn Single>().unwrap();
    resources.collect::<dyn Pair>().unwrap();
    assert_eq!(resources.get::<Seq<dyn Pair>>().unwrap().len(), 2);
}
