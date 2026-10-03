use std::sync::Arc;

use anyhow::anyhow;
use dioxus::CapturedError;
use haze::{Later, Pack, Resources};

#[derive(Clone)]
struct Motto(&'static str);

#[derive(Clone)]
struct Keeper;

macro_rules! pack {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Pack)]
        struct $name {
            $($field: $ty),*
        }
    };
}

pack!(Made { name: String, motto: Option<Motto>, keeper: Later<Keeper> });

macro_rules! pack_of {
    ($name:ident, $inner:ident) => {
        #[derive(Clone, Pack)]
        struct $name {
            inner: $inner,
            motto: Option<$inner>,
        }
    };
}

pack_of!(Held, Motto);

#[derive(Clone)]
struct Length(usize);

#[derive(Clone)]
struct Theme(&'static str);

macro_rules! resource_fn {
    ($name:ident($arg:ident: $ty:ty) -> $ret:ty { $body:expr }) => {
        #[haze::resource]
        fn $name($arg: $ty) -> $ret {
            $body
        }
    };
}

resource_fn!(length(text: String) -> Length { Length(text.len()) });

resource_fn!(theme(motto: Motto) -> Result<Theme, CapturedError> {
    if motto.0.is_empty() {
        Err(CapturedError(Arc::new(anyhow!("the motto is empty"))))
    } else {
        Ok(Theme(motto.0))
    }
});

#[tokio::test]
async fn types_from_macro_rules_fragments_are_classified_like_written_ones() {
    let resources = Resources::start(async |resources| {
        resources.insert(String::from("ana"));
        resources.insert(Motto("onward"));
        resources.insert(Keeper);
        Ok(())
    })
    .await
    .unwrap();
    let made = resources.get::<Made>().unwrap();
    assert_eq!(made.name, "ana");
    assert_eq!(made.motto.map(|motto| motto.0), Some("onward"));
    assert!(made.keeper.get().is_ok());
    let held = resources.get::<Held>().unwrap();
    assert_eq!(held.inner.0, "onward");
    assert_eq!(held.motto.map(|motto| motto.0), Some("onward"));
    assert_eq!(resources.get::<Length>().unwrap().0, 3);
    assert_eq!(resources.get::<Theme>().unwrap().0, "onward");
    assert!(!resources.contains::<Result<Theme, CapturedError>>());
}
