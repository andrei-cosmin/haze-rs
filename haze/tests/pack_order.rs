use haze::{Pack, Resources};

#[derive(Clone, Pack)]
struct Alpha {
    beta: Option<Beta>,
}

#[derive(Clone, Pack)]
struct Beta {
    name: String,
}

#[tokio::test]
async fn packs_are_built_in_name_order() {
    let resources = Resources::start(async |resources| {
        resources.insert(String::from("ana"));
        Ok(())
    })
    .await
    .unwrap();
    assert!(resources.get::<Alpha>().unwrap().beta.is_none());
    assert_eq!(resources.get::<Beta>().unwrap().name, "ana");
}
