//! Port of `Timing.java`: optional per-thread phase observer.

use std::cell::RefCell;

/// `Timing.observer` callback type (`Consumer<String>`).
pub type Observer = Box<dyn Fn(&str)>;

thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}

/// `Timing.observer.set(sink)`; returns the previous observer.
pub fn set_observer(sink: Option<Observer>) -> Option<Observer> {
    OBSERVER.with(|o| std::mem::replace(&mut *o.borrow_mut(), sink))
}

/// The name of a measured phase, such as `"geometry_begin"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingPhase(pub &'static str);

/// `Timing.mark(phase)`.
pub fn mark(phase: TimingPhase) {
    let phase = phase.0;
    OBSERVER.with(|o| {
        if let Some(sink) = o.borrow().as_ref() {
            sink(phase);
        }
    });
}
