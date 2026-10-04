//! The recorder that the hook tests render their components with.

use std::{cell::RefCell, rc::Rc, time::Duration};

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
    pub(crate) fn items(&self) -> Vec<u32> {
        self.items.borrow().clone()
    }

    pub(crate) fn gaps(&self) -> Vec<u64> {
        let opened = self.opened.borrow();
        let mut gaps = Vec::new();
        for pair in opened.windows(2) {
            gaps.push((pair[1] - pair[0]).as_secs());
        }
        gaps
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
