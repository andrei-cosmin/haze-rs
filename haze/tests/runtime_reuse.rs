#![cfg(feature = "standalone")]

use std::{sync::mpsc, thread, time::Duration};

use tokio::runtime::{Builder, Handle};

#[test]
fn install_runs_startup_on_the_runtime_the_caller_entered() {
    let app = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("app")
        .enable_all()
        .build()
        .unwrap();
    let _guard = app.enter();

    let resources = haze::install(async |resources| {
        resources.insert(Handle::current());
        Ok(())
    });

    let handle = resources.get::<Handle>().unwrap();
    let (sender, receiver) = mpsc::channel();
    handle.spawn(async move {
        sender
            .send(thread::current().name().map(str::to_owned))
            .unwrap();
    });
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(5)),
        Ok(Some(String::from("app")))
    );
}
