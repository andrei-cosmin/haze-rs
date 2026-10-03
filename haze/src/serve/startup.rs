//! Runs the app's startup once and hands the same registry to every router.

use std::cell::Cell;

use anyhow::Result;
use tokio::sync::OnceCell;

use crate::Resources;

/// The startup `serve` runs on first start and reuses after every server
/// hot-patch, built on tokio's [`OnceCell::get_or_try_init`].
pub(crate) struct Startup<S> {
    /// The app's setup, taken on the first and only run.
    setup: Cell<Option<S>>,
    /// The registry built by that run.
    built: OnceCell<Resources>,
}

impl<S> Startup<S>
where
    S: AsyncFnOnce(&mut Resources) -> Result<()>,
{
    /// Holds `setup` until the first router asks for resources.
    pub(crate) fn new(setup: S) -> Self {
        Self {
            setup: Cell::new(Some(setup)),
            built: OnceCell::new(),
        }
    }

    /// Runs [`Resources::start`] on the first call and returns the same
    /// registry on every later one.
    pub(crate) async fn resources(&self) -> Result<Resources> {
        let built = self
            .built
            .get_or_try_init(|| async {
                let Some(setup) = self.setup.take() else {
                    unreachable!("Dioxus stops the server when the first setup fails");
                };
                Resources::start(setup).await
            })
            .await?;
        Ok(built.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use anyhow::anyhow;

    use super::Startup;
    use crate::Resources;

    #[tokio::test]
    async fn setup_runs_once_and_every_router_gets_the_same_registry() {
        let runs = Rc::new(Cell::new(0));
        let counter = Rc::clone(&runs);
        let startup = Startup::new(async move |resources: &mut Resources| {
            counter.set(counter.get() + 1);
            resources.insert(7_u64);
            Ok(())
        });
        let first = startup.resources().await.unwrap();
        let second = startup.resources().await.unwrap();
        assert_eq!(runs.get(), 1);
        assert_eq!(
            (first.get::<u64>(), second.get::<u64>()),
            (Some(7), Some(7))
        );
    }

    #[tokio::test]
    async fn a_failing_setup_is_reported() {
        let startup = Startup::new(async |_: &mut Resources| Err(anyhow!("database is locked")));
        let Err(error) = startup.resources().await else {
            panic!("startup succeeded although setup failed");
        };
        assert_eq!(error.to_string(), "database is locked");
    }
}
