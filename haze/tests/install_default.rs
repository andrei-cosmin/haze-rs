use std::sync::Arc;

use haze::Resources;

#[test]
fn the_first_installed_registry_is_the_process_default_and_later_ones_are_handed_back() {
    assert!(Resources::get_default().is_none());

    let counter = Arc::new(7_u64);
    let mut first = Resources::new();
    first.insert(Arc::clone(&counter));
    first.install_default().unwrap();

    let installed = Resources::get_default().unwrap();
    assert!(Arc::ptr_eq(&installed.get::<Arc<u64>>().unwrap(), &counter));

    let mut second = Resources::new();
    second.insert(String::from("second"));
    let rejected = second.install_default().unwrap_err();
    assert_eq!(rejected.get::<String>().as_deref(), Some("second"));

    let still = Resources::get_default().unwrap();
    assert!(still.get::<String>().is_none());
    assert!(Arc::ptr_eq(&still.get::<Arc<u64>>().unwrap(), &counter));
}
