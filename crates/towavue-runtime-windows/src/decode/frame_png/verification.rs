//! Test-only observation of the noninterruptible native PNG encoding boundary.
//! None follows the last pre-encode cancellation check; Some records actual
//! compressed packet bytes before the post-encode cancellation check.
use std::{cell::RefCell, rc::Rc};

type Observer = Rc<dyn Fn(Option<usize>)>;
thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}

struct Restore(Option<Observer>);
impl Drop for Restore {
    fn drop(&mut self) {
        OBSERVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

pub(crate) fn with_encoder_observer<R>(
    observer: impl Fn(Option<usize>) + 'static,
    work: impl FnOnce() -> R,
) -> R {
    let _restore = Restore(OBSERVER.with(|slot| slot.replace(Some(Rc::new(observer)))));
    work()
}

pub(super) fn observe(bytes: Option<usize>) {
    // Release the slot borrow before invoking test code; nested scopes restore
    // their predecessor, and independent test threads cannot intercept this job.
    let observer = OBSERVER.with(|slot| slot.borrow().clone());
    if let Some(observer) = observer {
        observer(bytes);
    }
}
