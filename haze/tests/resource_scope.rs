use anyhow::{Result, anyhow};
use haze::{Later, Resources};

trait Lookup {
    fn try_get<T>(&self) -> Result<T> {
        Err(anyhow!("hijacked"))
    }

    fn later<T>(&self) -> Option<T> {
        None
    }
}

impl<X> Lookup for X {}

trait Ext {
    fn get<T>(&mut self) -> Option<T>;
}

impl Ext for Resources {
    fn get<T>(&mut self) -> Option<T> {
        None
    }
}

#[derive(Clone)]
struct Motto(&'static str);

#[derive(Clone)]
struct Banner(Option<&'static str>);

#[haze::resource]
fn banner(motto: Option<Motto>) -> Banner {
    Banner(motto.map(|motto| motto.0))
}

#[derive(Clone)]
struct Required(&'static str);

#[haze::resource]
fn required(motto: Motto) -> Required {
    Required(motto.0)
}

#[derive(Clone)]
struct Kept(Later<Motto>);

#[haze::resource]
fn kept(motto: Later<Motto>) -> Kept {
    Kept(motto)
}

#[tokio::test]
async fn traits_in_scope_with_registry_method_names_leave_resource_functions_alone() {
    let mut premise = Resources::new();
    premise.insert(Motto("x"));
    let registry = &mut premise;
    assert!(registry.get::<Motto>().is_none());
    assert!(registry.try_get::<Motto>().is_err());
    assert!(registry.later::<Motto>().is_none());
    let resources = Resources::start(async |resources| {
        resources.insert(Motto("onward"));
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(resources.get::<Banner>().unwrap().0, Some("onward"));
    assert_eq!(resources.get::<Required>().unwrap().0, "onward");
    assert_eq!(
        resources.get::<Kept>().unwrap().0.get().unwrap().0,
        "onward"
    );
}
