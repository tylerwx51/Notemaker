//! `Field<T>`: a Java instance field shared between threads.
//!
//! Java code freely reads and writes fields from the UI thread and background
//! threads (often inside `synchronized(lock)`). Each ported field is wrapped
//! in a `Field`, whose accessors take a short reentrant per-field lock. A
//! nested *mutable* access to the same field from one thread (only possible
//! by calling back into code that touches the field from inside `with_mut`)
//! panics with the field type instead of deadlocking; keep closures leaf-level.

use crate::monitor::Monitor;
use std::cell::RefCell;
use std::fmt;

pub struct Field<T> {
    lock: Monitor,
    value: RefCell<T>,
}
// Safety: every access to `value` holds `lock`, which admits one thread at a
// time; RefCell guards reentrant access from that thread.
unsafe impl<T: Send> Sync for Field<T> {}
unsafe impl<T: Send> Send for Field<T> {}

impl<T> Field<T> {
    pub const fn new(value: T) -> Self {
        Field { lock: Monitor::new(), value: RefCell::new(value) }
    }
    /// Reads through a closure.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let _g = self.lock.enter();
        let v = self.value.try_borrow().unwrap_or_else(|_| panic!("Field<{}> read while being modified", std::any::type_name::<T>()));
        f(&v)
    }
    /// Mutates through a closure.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let _g = self.lock.enter();
        let mut v = self.value.try_borrow_mut().unwrap_or_else(|_| panic!("Field<{}> modified reentrantly", std::any::type_name::<T>()));
        f(&mut v)
    }
    /// Assignment (`this.x = value`).
    pub fn set(&self, value: T) {
        self.with_mut(|v| *v = value)
    }
    /// Replaces the value, returning the old one.
    pub fn replace(&self, value: T) -> T {
        self.with_mut(|v| std::mem::replace(v, value))
    }
}
impl<T: Clone> Field<T> {
    /// Reads a copy (`x = this.field`). For Java reference types (`Obj`,
    /// `Arc`, `JSONObject`) this copies the reference, as Java does.
    pub fn get(&self) -> T {
        self.with(|v| v.clone())
    }
}
impl<T: Default> Field<T> {
    pub fn take(&self) -> T {
        self.with_mut(std::mem::take)
    }
}
impl<T: Default> Default for Field<T> {
    fn default() -> Self {
        Field::new(T::default())
    }
}
impl<T: fmt::Debug> fmt::Debug for Field<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.with(|v| v.fmt(f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reentrant_reads() {
        let f = Field::new(vec![1, 2]);
        f.with(|a| f.with(|b| assert_eq!(a, b)));
        f.with_mut(|v| v.push(3));
        assert_eq!(f.get(), vec![1, 2, 3]);
    }
}
