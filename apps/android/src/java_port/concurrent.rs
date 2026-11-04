//! `java.util.concurrent` pieces the app uses: single-thread executors
//! (`Executors.newSingleThreadExecutor`), `Future.get` and `CountDownLatch`.
//!
//! Workers run as [`JThread`]s so `shutdownNow()` interrupts them the way
//! Java did (cooperatively: `JThread::sleep` and `JThread::interrupted`).

use crate::jx::{JThread, JavaError};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

type Task = Box<dyn FnOnce() + Send + 'static>;

#[derive(Default)]
struct Queue {
    tasks: VecDeque<Task>,
    shutdown: bool,
    worker: Option<JThread>,
}

/// `ExecutorService` backed by one lazily started, named worker thread.
pub struct ExecutorService {
    name: String,
    state: Arc<(Mutex<Queue>, Condvar)>,
}

static POOLS: AtomicUsize = AtomicUsize::new(0);

impl ExecutorService {
    /// `Executors.newSingleThreadExecutor(r -> new Thread(r, name))`.
    pub fn named(name: &str) -> ExecutorService {
        ExecutorService { name: name.into(), state: Arc::new((Mutex::new(Queue::default()), Condvar::new())) }
    }
    /// `Executors.newSingleThreadExecutor()` (thread `pool-N-thread-1`).
    pub fn single_thread() -> ExecutorService {
        let n = POOLS.fetch_add(1, Ordering::SeqCst) + 1;
        ExecutorService::named(&format!("pool-{n}-thread-1"))
    }

    /// `execute(task)`. After `shutdown_now` the task is rejected silently
    /// (Java threw `RejectedExecutionException`; callers never relied on it).
    pub fn execute(&self, task: impl FnOnce() + Send + 'static) {
        let (lock, cv) = &*self.state;
        let mut q = lock.lock().unwrap();
        if q.shutdown {
            crate::jx::log::w(crate::jx::log::NOTEMAKER, &format!("Task rejected by shut down executor {}", self.name));
            return;
        }
        q.tasks.push_back(Box::new(task));
        if q.worker.is_none() {
            let state = self.state.clone();
            q.worker = Some(JThread::spawn(&crate::jx::ThreadName::from(&self.name), move || worker(state)));
        }
        cv.notify_all();
    }

    /// `submit(callable)`: the returned future yields the task's result.
    pub fn submit<T: Send + 'static>(&self, task: impl FnOnce() -> T + Send + 'static) -> Future<T> {
        let future = Future::new();
        let done = future.clone();
        self.execute(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
            done.complete(result.map_err(|p| {
                let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
                JavaError::new("java.util.concurrent.ExecutionException", msg)
            }));
        });
        future
    }

    /// `shutdownNow()`: drops queued tasks and interrupts the worker.
    pub fn shutdown_now(&self) {
        let (lock, cv) = &*self.state;
        let mut q = lock.lock().unwrap();
        q.shutdown = true;
        q.tasks.clear();
        if let Some(w) = &q.worker {
            w.interrupt();
        }
        cv.notify_all();
    }
    pub fn is_shutdown(&self) -> bool {
        self.state.0.lock().unwrap().shutdown
    }
}

fn worker(state: Arc<(Mutex<Queue>, Condvar)>) {
    let (lock, cv) = &*state;
    loop {
        let task = {
            let mut q = lock.lock().unwrap();
            loop {
                if let Some(t) = q.tasks.pop_front() {
                    break t;
                }
                if q.shutdown {
                    return;
                }
                q = cv.wait(q).unwrap();
            }
        };
        // Java's worker survives a throwing task; a panic here would kill the
        // only worker, so it is logged and the loop continues.
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::jx::frame(task))) {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            crate::jx::log::e(crate::jx::log::NOTEMAKER, &format!("Uncaught failure in executor task: {msg}"));
        }
    }
}

/// `Future<T>` from [`ExecutorService::submit`].
pub struct Future<T> {
    state: Arc<(Mutex<Option<Result<T, JavaError>>>, Condvar)>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
impl<T> Clone for Future<T> {
    fn clone(&self) -> Self {
        Future { state: self.state.clone(), cancelled: self.cancelled.clone() }
    }
}
impl<T> Future<T> {
    pub fn new() -> Future<T> {
        Future { state: Arc::new((Mutex::new(None), Condvar::new())), cancelled: Default::default() }
    }
    pub fn complete(&self, value: Result<T, JavaError>) {
        let (lock, cv) = &*self.state;
        *lock.lock().unwrap() = Some(value);
        cv.notify_all();
    }
    /// `get()`: blocks until done. A panicking task yields `ExecutionException`.
    pub fn get(&self) -> Result<T, JavaError> {
        let (lock, cv) = &*self.state;
        let mut v = lock.lock().unwrap();
        loop {
            if let Some(r) = v.take() {
                return r;
            }
            v = cv.wait(v).unwrap();
        }
    }
    /// `get(timeout, unit)`: `TimeoutException` after `timeout`.
    pub fn get_timeout(&self, timeout: Duration) -> Result<T, JavaError> {
        let (lock, cv) = &*self.state;
        let deadline = Instant::now() + timeout;
        let mut v = lock.lock().unwrap();
        loop {
            if let Some(r) = v.take() {
                return r;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(JavaError::new("java.util.concurrent.TimeoutException", ""));
            }
            v = cv.wait_timeout(v, deadline - now).unwrap().0;
        }
    }
    pub fn is_done(&self) -> bool {
        self.state.0.lock().unwrap().is_some()
    }
    /// `cancel(false)`: marks the future cancelled; a running task completes.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
impl<T> Default for Future<T> {
    fn default() -> Self {
        Future::new()
    }
}

/// `java.util.concurrent.CountDownLatch`.
pub struct CountDownLatch {
    count: Mutex<i64>,
    cv: Condvar,
}
impl CountDownLatch {
    pub fn new(count: i64) -> CountDownLatch {
        CountDownLatch { count: Mutex::new(count), cv: Condvar::new() }
    }
    pub fn count_down(&self) {
        let mut c = self.count.lock().unwrap();
        if *c > 0 {
            *c -= 1;
            if *c == 0 {
                self.cv.notify_all();
            }
        }
    }
    pub fn get_count(&self) -> i64 {
        *self.count.lock().unwrap()
    }
    /// `await()`.
    pub fn await_(&self) {
        let mut c = self.count.lock().unwrap();
        while *c > 0 {
            c = self.cv.wait(c).unwrap();
        }
    }
    /// `await(timeout, unit)`: false on timeout.
    pub fn await_timeout(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut c = self.count.lock().unwrap();
        while *c > 0 {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            c = self.cv.wait_timeout(c, deadline - now).unwrap().0;
        }
        true
    }
}
