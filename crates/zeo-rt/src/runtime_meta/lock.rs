//! The overlay's locks, and the one rule they enforce.
//!
//! `std::sync::RwLock` is QUEUED: a waiting writer blocks new readers. So a
//! thread that reads a lock it already holds deadlocks the instant another
//! thread queues a write between the two acquisitions -- and only then, which
//! is why this class of bug reaches production. Single-threaded there is no
//! waiting writer and the same code runs fine every time.
//!
//! The module header states the contract ("No lock is ever held across a
//! dispatched call"), and a contract nothing checks decays. [`OverlayLock`]
//! checks it: in a debug build every acquisition records the lock on a
//! thread-local list and PANICS if it is already there, naming the field. The
//! recording compiles out of release, where the guard is the std guard and a
//! zero-sized field.
//!
//! The rule this enforces is not "do not deadlock". It is stronger and much
//! easier to obey: **take what you need out from under the guard, drop it, and
//! only then call anything.** Every accessor in this module already did that;
//! four had drifted.

use std::ops::{Deref, DerefMut};
use std::sync::{LockResult, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(debug_assertions)]
thread_local! {
    /// The overlay locks this thread holds. A `Vec` and a linear scan: the
    /// depth is one, two at the very most, so anything cleverer costs more
    /// than it saves.
    static HELD: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// One thread's claim on one lock, released when the guard drops. A ZST in
/// release.
#[cfg(debug_assertions)]
struct Ticket(usize);

#[cfg(not(debug_assertions))]
struct Ticket;

#[cfg(debug_assertions)]
impl Ticket {
    fn claim(key: usize, name: &'static str, how: &'static str) -> Ticket {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            assert!(
                !held.contains(&key),
                "overlay lock `{name}` re-entered ({how}): this thread already holds it. \
                 std's RwLock is queued, so a writer waiting between the two acquisitions \
                 deadlocks the process -- clone what is needed out and DROP the guard \
                 before the call that comes back here."
            );
            held.push(key);
        });
        Ticket(key)
    }
}

#[cfg(not(debug_assertions))]
impl Ticket {
    #[inline(always)]
    fn claim(_key: usize, _name: &'static str, _how: &'static str) -> Ticket {
        Ticket
    }
}

#[cfg(debug_assertions)]
impl Drop for Ticket {
    fn drop(&mut self) {
        // Not `pop`: guards need not drop in acquisition order. The key is
        // unique in the list by the very invariant this type enforces, so
        // removing it by value is exact.
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(i) = held.iter().rposition(|&k| k == self.0) {
                held.remove(i);
            }
        });
    }
}

/// An `RwLock` that refuses, in a debug build, to be taken twice by one
/// thread. See the module docs.
pub(crate) struct OverlayLock<T> {
    inner: RwLock<T>,
    /// The field name, so the panic names the lock rather than an address.
    name: &'static str,
}

impl<T> OverlayLock<T> {
    pub(crate) const fn new(name: &'static str, value: T) -> OverlayLock<T> {
        OverlayLock {
            inner: RwLock::new(value),
            name,
        }
    }

    /// The identity of THIS lock -- its own address, so two fields of one
    /// `OverlayMaps` are distinct claims.
    #[inline(always)]
    fn key(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }

    pub(crate) fn read(&self) -> LockResult<ReadGuard<'_, T>> {
        let ticket = Ticket::claim(self.key(), self.name, "read");
        match self.inner.read() {
            Ok(inner) => Ok(ReadGuard {
                inner,
                _ticket: ticket,
            }),
            Err(e) => Err(PoisonError::new(ReadGuard {
                inner: e.into_inner(),
                _ticket: ticket,
            })),
        }
    }

    pub(crate) fn write(&self) -> LockResult<WriteGuard<'_, T>> {
        let ticket = Ticket::claim(self.key(), self.name, "write");
        match self.inner.write() {
            Ok(inner) => Ok(WriteGuard {
                inner,
                _ticket: ticket,
            }),
            Err(e) => Err(PoisonError::new(WriteGuard {
                inner: e.into_inner(),
                _ticket: ticket,
            })),
        }
    }
}

pub(crate) struct ReadGuard<'a, T> {
    inner: RwLockReadGuard<'a, T>,
    _ticket: Ticket,
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        &self.inner
    }
}

pub(crate) struct WriteGuard<'a, T> {
    inner: RwLockWriteGuard<'a, T>,
    _ticket: Ticket,
}

impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T> DerefMut for WriteGuard<'_, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::OverlayLock;

    #[test]
    fn a_second_read_on_one_thread_is_refused() {
        let lock = OverlayLock::new("test", 1u32);
        let _outer = lock.read().unwrap();
        let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lock.read().unwrap()));
        assert!(again.is_err(), "a recursive read must not be allowed");
    }

    #[test]
    fn two_locks_are_two_claims() {
        let a = OverlayLock::new("a", 1u32);
        let b = OverlayLock::new("b", 2u32);
        let ga = a.read().unwrap();
        let gb = b.read().unwrap();
        assert_eq!((*ga, *gb), (1, 2));
    }

    #[test]
    fn a_claim_is_released_when_its_guard_drops() {
        let lock = OverlayLock::new("test", 1u32);
        drop(lock.read().unwrap());
        drop(lock.write().unwrap());
        // A second sequential pair proves the release is not one-shot.
        assert_eq!(*lock.read().unwrap(), 1);
    }
}
