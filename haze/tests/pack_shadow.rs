use haze::{Pack, Resources};

#[derive(Clone)]
struct Store(&'static str);

#[derive(Clone, Pack)]
struct Shelf {
    resources: Store,
    name: String,
}

#[tokio::test]
async fn a_field_named_resources_does_not_shadow_the_registry() {
    let resources = Resources::start(async |resources| {
        resources.insert(Store("books"));
        resources.insert(String::from("ana"));
        Ok(())
    })
    .await
    .unwrap();
    let shelf = resources.get::<Shelf>().unwrap();
    assert_eq!(shelf.resources.0, "books");
    assert_eq!(shelf.name, "ana");
}
