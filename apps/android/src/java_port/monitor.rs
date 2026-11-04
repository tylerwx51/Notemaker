//! Java monitor semantics (`synchronized`, `wait`, `notify`, `notifyAll`):
//! reentrant, owned by a thread, with condition waiting that releases every
//! recursive hold and restores it afterwards.

use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    owner: Option<ThreadId>,
    count: usize,
    /// Incremented by notify/notifyAll so waiters can tell they were signalled.
    signals: u64,
}

#[derive(Default)]
pub struct Monitor {
    state: Mutex<State>,
    entry: Condvar,
    waiters: Condvar,
}

pub struct Guard<'a> {
    monitor: &'a Monitor,
}

impl Monitor {
    pub const fn new() -> Self {
        Monitor { state: Mutex::new(State { owner: None, count: 0, signals: 0 }), entry: Condvar::new(), waiters: Condvar::new() }
    }
    /// `synchronized (this) { ... }`: hold the returned guard for the block.
    pub fn enter(&self) -> Guard<'_> {
        let me = std::thread::current().id();
        let mut s = self.state.lock().unwrap();
        loop {
            match s.owner {
                None => {
                    s.owner = Some(me);
                    s.count = 1;
                    break;
                }
                Some(o) if o == me => {
                    s.count += 1;
                    break;
                }
                _ => s = self.entry.wait(s).unwrap(),
            }
        }
        Guard { monitor: self }
    }
    /// Runs `f` while holding the monitor.
    pub fn sync<T>(&self, f: impl FnOnce() -> T) -> T {
        let _g = self.enter();
        f()
    }
    pub fn held_by_current_thread(&self) -> bool {
        self.state.lock().unwrap().owner == Some(std::thread::current().id())
    }
    /// `Object.wait(timeout)`; `timeout_ms == 0` waits indefinitely. Must hold the monitor.
    pub fn wait(&self, timeout_ms: u64) {
        let me = std::thread::current().id();
        let mut s = self.state.lock().unwrap();
        assert_eq!(s.owner, Some(me), "IllegalMonitorStateException");
        let held = s.count;
        s.owner = None;
        s.count = 0;
        let start_signal = s.signals;
        self.entry.notify_one();
        let deadline = (timeout_ms > 0).then(|| Instant::now() + Duration::from_millis(timeout_ms));
        while s.signals == start_signal {
            match deadline {
                None => s = self.waiters.wait(s).unwrap(),
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        break;
                    }
                    s = self.waiters.wait_timeout(s, d - now).unwrap().0;
                }
            }
        }
        while s.owner.is_some() {
            s = self.entry.wait(s).unwrap();
        }
        s.owner = Some(me);
        s.count = held;
    }
    pub fn notify_all(&self) {
        let mut s = self.state.lock().unwrap();
        s.signals += 1;
        self.waiters.notify_all();
    }
    /// Java permits waking more than one waiter for `notify`; waking all is a valid implementation.
    pub fn notify(&self) {
        self.notify_all()
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let mut s = self.monitor.state.lock().unwrap();
        s.count -= 1;
        if s.count == 0 {
            s.owner = None;
            self.monitor.entry.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn reentrant_and_wait() {
        let m = Arc::new(Monitor::new());
        let g1 = m.enter();
        let g2 = m.enter();
        drop(g2);
        let m2 = m.clone();
        let t = std::thread::spawn(move || {
            let _g = m2.enter();
            m2.notify_all();
        });
        m.wait(5000);
        drop(g1);
        t.join().unwrap();
    }
}
