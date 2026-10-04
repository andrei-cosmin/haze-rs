use std::{
    any::type_name,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use haze::{Later, Pack, Resources};

struct Copied(Arc<AtomicU64>);

impl Clone for Copied {
    fn clone(&self) -> Self {
        self.0.fetch_add(1, Ordering::Relaxed);
        Self(Arc::clone(&self.0))
    }
}

#[derive(Clone, Pack)]
struct Ledger {
    early: Later<Copied>,
    copied: Copied,
    tail: Tail,
}

#[derive(Clone, Pack)]
struct Tail {
    name: String,
}

#[tokio::test]
async fn a_pack_that_waits_a_round_copies_each_field_once() {
    assert!(type_name::<Ledger>() < type_name::<Tail>());
    let copies = Arc::new(AtomicU64::new(0));
    let resources = Resources::start(async |resources| {
        resources.insert(Copied(Arc::clone(&copies)));
        resources.insert(String::from("ana"));
        Ok(())
    })
    .await
    .unwrap();
    let ledger = resources.get::<Ledger>().unwrap();
    assert_eq!(ledger.tail.name, "ana");
    assert!(Arc::ptr_eq(&ledger.copied.0, &copies));
    assert!(Arc::ptr_eq(&ledger.early.get().unwrap().0, &copies));
    assert_eq!(copies.load(Ordering::Relaxed), 2);
}
