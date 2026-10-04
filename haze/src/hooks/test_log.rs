//! The recorder that the hook tests render their components with.

use std::{
    cell::{Ref, RefCell},
    rc::Rc,
    time::Duration,
};

use dioxus::{core::NoOpMutations, prelude::*};
use futures_util::{Stream, StreamExt, stream};
use tokio::time::Instant;

use crate::hooks::connection::Connection;

#[derive(Clone, Default)]
pub(crate) struct Log {
    pub(crate) items: Rc<RefCell<Vec<u32>>>,
    pub(crate) states: Rc<RefCell<Vec<Connection>>>,
    pub(crate) opened: Rc<RefCell<Vec<Instant>>>,
    pub(crate) handles: Rc<RefCell<Vec<ReadSignal<Connection>>>>,
}

impl PartialEq for Log {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }
}

impl Log {
    const POLL: Duration = Duration::from_millis(150);

    pub(crate) fn items(&self) -> Ref<'_, Vec<u32>> {
        self.items.borrow()
    }

    pub(crate) fn gaps(&self) -> Vec<u64> {
        let opened = self.opened.borrow();
        opened
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).as_secs())
            .collect()
    }

    pub(crate) fn spacing(&self) -> Vec<Duration> {
        let opened = self.opened.borrow();
        opened.windows(2).map(|pair| pair[1] - pair[0]).collect()
    }

    pub(crate) fn within_jitter(spacing: &[Duration], full: &[u64]) -> bool {
        spacing.iter().zip(full).all(|(gap, &full)| {
            let full = Duration::from_secs(full);
            *gap >= full / 2 && *gap < full + Self::POLL
        })
    }

    pub(crate) fn record(&self, status: Connection) {
        let mut seen = self.states.borrow_mut();
        if seen.last() != Some(&status) {
            seen.push(status);
        }
    }

    pub(crate) fn watch(&self, status: ReadSignal<Connection>) {
        self.handles.borrow_mut().push(status);
        self.record(status());
    }

    pub(crate) fn same_handle_every_render(&self) -> bool {
        let handles = self.handles.borrow();
        handles.len() >= 3 && handles.iter().all(|handle| Some(handle) == handles.first())
    }

    pub(crate) fn one_now_two_later() -> impl Stream<Item = u32> {
        let two = stream::once(async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            2
        });
        stream::iter([1]).chain(two).chain(stream::pending())
    }

    pub(crate) async fn drive(dom: &mut VirtualDom, until: impl Fn() -> bool) {
        dom.rebuild_in_place();
        for _ in 0..400 {
            if until() {
                return;
            }
            tokio::select! {
                () = dom.wait_for_work() => {}
                () = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            dom.render_immediate(&mut NoOpMutations);
        }
        panic!("the hook never reached the expected state");
    }
}
