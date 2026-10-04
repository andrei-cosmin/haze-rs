use anyhow::Result;
use haze::{Build, Later, Pack, Resources, Seq};

trait Greeter: Send + Sync {
    fn greet(&self) -> String;
}

#[derive(Clone, Pack)]
struct Named {
    name: String,
}

#[haze::register(order = 10)]
impl Greeter for Named {
    fn greet(&self) -> String {
        format!("hi {}", self.name)
    }
}

struct Plain;

impl Build for Plain {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = 20)]
impl Greeter for Plain {
    fn greet(&self) -> String {
        String::from("hello")
    }
}

struct Early;

impl Build for Early {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = -5)]
impl Greeter for Early {
    fn greet(&self) -> String {
        String::from("first")
    }
}

#[tokio::test]
async fn collects_every_implementation_in_order() {
    let resources = Resources::start(async |resources| {
        resources.insert(String::from("ana"));
        resources.insert(Shared(String::from("shared")));
        Ok(())
    })
    .await
    .unwrap();
    let greeters = resources.get::<Seq<dyn Greeter>>().unwrap();
    let mut greetings = Vec::new();
    for greeter in &greeters {
        greetings.push(greeter.greet());
    }
    assert_eq!(greetings, ["first", "hi ana", "hello"]);
}

#[test]
fn a_trait_without_registrations_collects_nothing() {
    trait Unused: Send + Sync {}
    let mut resources = Resources::new();
    resources.collect::<dyn Unused>().unwrap();
    assert!(resources.get::<Seq<dyn Unused>>().unwrap().is_empty());
}

#[test]
fn a_pack_builds_by_hand_from_resources() {
    let mut resources = Resources::new();
    resources.insert(String::from("bo"));
    assert_eq!(Named::build(&resources).unwrap().name, "bo");
}

trait Tool: Send + Sync {
    fn name(&self) -> String;
}

#[derive(Clone)]
struct Shared(String);

#[haze::register(order = 1)]
impl Tool for Shared {
    fn name(&self) -> String {
        self.0.clone()
    }
}

#[derive(Clone, Pack)]
struct Assembled {
    label: String,
}

#[haze::register(order = 2)]
impl Tool for Assembled {
    fn name(&self) -> String {
        format!("built {}", self.label)
    }
}

#[test]
fn inserted_types_are_reused_even_when_they_are_packs() {
    let mut resources = Resources::new();
    resources.insert(Shared(String::from("shared")));
    resources.insert(String::from("x"));
    resources.insert(Assembled {
        label: String::from("inserted"),
    });
    resources.collect::<dyn Tool>().unwrap();
    let tools = resources.get::<Seq<dyn Tool>>().unwrap();
    let mut names = Vec::new();
    for tool in &tools {
        names.push(tool.name());
    }
    assert_eq!(names, ["shared", "built inserted"]);
}

#[test]
fn a_registered_type_that_was_never_inserted_names_itself() {
    let mut resources = Resources::new();
    resources.insert(String::from("x"));
    let error = resources.collect::<dyn Tool>().unwrap_err();
    assert!(format!("{error:#}").contains("Shared was never inserted"));
}

trait Voice: Send + Sync {
    fn say(&self) -> String;
}

struct Abe;

impl Build for Abe {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = 5)]
impl Voice for Abe {
    fn say(&self) -> String {
        String::from("abe")
    }
}

struct Zed;

impl Build for Zed {
    fn build(_resources: &Resources) -> Result<Self> {
        Ok(Self)
    }
}

#[haze::register(order = 5)]
impl Voice for Zed {
    fn say(&self) -> String {
        String::from("zed")
    }
}

#[test]
fn equal_orders_are_broken_by_type_name() {
    let mut resources = Resources::new();
    resources.collect::<dyn Voice>().unwrap();
    let voices = resources.get::<Seq<dyn Voice>>().unwrap();
    let mut said = Vec::new();
    for voice in &voices {
        said.push(voice.say());
    }
    assert_eq!(said, ["abe", "zed"]);
}

trait Chorus: Send + Sync {
    fn sing(&self) -> String;
}

#[derive(Clone, Pack)]
struct Choir {
    voices: Seq<dyn Voice>,
}

#[haze::register(order = 1)]
impl Chorus for Choir {
    fn sing(&self) -> String {
        let mut lines = Vec::new();
        for voice in &self.voices {
            lines.push(voice.say());
        }
        lines.join(", ")
    }
}

#[derive(Clone)]
struct Roster(Later<Seq<dyn Voice>>);

#[haze::resource]
fn roster(voices: Later<Seq<dyn Voice>>) -> Roster {
    Roster(voices)
}

#[tokio::test]
async fn start_collects_a_trait_whose_implementation_needs_another_traits_seq() {
    let resources = Resources::start(async |resources| {
        resources.insert(String::from("ana"));
        resources.insert(Shared(String::from("shared")));
        Ok(())
    })
    .await
    .unwrap();
    let choruses = resources.get::<Seq<dyn Chorus>>().unwrap();
    assert_eq!(choruses[0].sing(), "abe, zed");
    let roster = resources.get::<Roster>().unwrap();
    assert_eq!(roster.0.get().unwrap().len(), 2);
}

#[tokio::test]
async fn start_keeps_a_seq_inserted_in_setup_and_builds_nothing_for_it() {
    let resources = Resources::start(async |resources| {
        resources.insert(String::from("ana"));
        let tools: Vec<Box<dyn Tool>> = Vec::new();
        resources.insert(Seq::from(tools));
        let voices: Vec<Box<dyn Voice>> = vec![Box::new(Zed)];
        resources.insert(Seq::from(voices));
        Ok(())
    })
    .await
    .unwrap();
    let choruses = resources.get::<Seq<dyn Chorus>>().unwrap();
    assert_eq!(choruses[0].sing(), "zed");
    assert!(resources.get::<Seq<dyn Tool>>().unwrap().is_empty());
}

#[tokio::test]
async fn start_names_the_first_implementation_of_each_trait_that_cannot_be_obtained() {
    let Err(error) = Resources::start(async |_| Ok(())).await else {
        panic!("start succeeded although register::Named cannot be built");
    };
    assert_eq!(
        format!("{error:#}"),
        "collecting register::Named as dyn register::Greeter: register::Named was never inserted; collecting register::Shared as dyn register::Tool: register::Shared was never inserted; register::Assembled needs alloc::string::String, which was never inserted; register::Named needs alloc::string::String, which was never inserted"
    );
}
