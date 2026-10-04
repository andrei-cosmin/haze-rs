use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use haze::{Pack, Resources};

#[derive(Clone)]
struct Counted(u64);

impl Counted {
    fn open(opens: &Arc<AtomicU64>) -> Self {
        Self(opens.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

#[derive(Clone, Pack)]
struct Pool {
    opens: Arc<AtomicU64>,
    #[pack(func = Counted::open(&opens))]
    counted: Counted,
    zeta: Zeta,
}

#[derive(Clone, Pack)]
struct Zeta {
    name: String,
}

#[tokio::test]
async fn a_func_field_runs_once_even_when_the_pack_waits_a_round() {
    let resources = Resources::start(async |resources| {
        resources.insert(Arc::new(AtomicU64::new(0)));
        resources.insert(String::from("ana"));
        Ok(())
    })
    .await
    .unwrap();
    let pool = resources.get::<Pool>().unwrap();
    assert_eq!(pool.zeta.name, "ana");
    assert_eq!(pool.counted.0, 1);
    assert_eq!(pool.opens.load(Ordering::Relaxed), 1);
}
