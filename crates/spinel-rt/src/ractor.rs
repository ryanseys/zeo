//! `Ractor` (Phase 13.8) -- real OS threads (`std::thread::spawn`, NOT may
//! coroutines: a Ractor's whole point is genuine parallelism regardless of
//! the GVL-emulating scheduler config) sharing the SAME global heap.
//! CRuby's real Ractors already share classes, methods, and the Symbol
//! table process-wide -- and this runtime's class registry/Symbol interner
//! are already global (`OnceLock`/`LazyLock`, Part 9) -- so no separate
//! namespace or serialization subsystem exists or is needed. Memory safety
//! is unconditional (`Arc`/`parking_lot::Mutex` everywhere); what Ractor
//! adds is the Ruby-SEMANTIC isolation discipline, enforced at the
//! boundaries:
//!
//! - **Everything crossing a boundary** (`Ractor.new` args, `#send`
//!   payloads, the block's final value at `#value`/`#join`) is either
//!   SHAREABLE -- deeply frozen, or inherently immutable -- and passes by
//!   reference (a cheap `Arc` clone), or gets DEEP-COPIED (`Str`/`Array`/
//!   `Hash`/`Range` rebuild recursively). An unfrozen user OBJECT is
//!   rejected with a catchable error instead of copied -- this runtime has
//!   no by-name ivar-SETTING reflection to rebuild one (a documented,
//!   narrower-than-CRuby divergence: real Ruby deep-clones it; here,
//!   `Ractor.make_shareable` it first). `Proc`/`Fiber`/`Thread`/`Mutex`/
//!   `Queue`/`MatchData` are rejected outright, like CRuby's own
//!   unshareables.
//! - **Block isolation is checked at COMPILE time** (`codegen::call`'s
//!   `Ractor.new` interception: the block's capture set must be empty) --
//!   strictly earlier than CRuby's own Proc-creation-time
//!   `Ractor::IsolationError`, one of the places AOT is simply better
//!   positioned.
//!
//! Documented divergences (all narrow, all loud rather than silently
//! wrong): globals/cvars stay process-shared (CRuby raises IsolationError
//! on non-main-Ractor access; here they genuinely share -- flagged for the
//! `Ruby::Box` work, which owns namespace isolation); an uncaught exception
//! re-raises AT `#value`/`#join` directly, like `Thread`, instead of
//! wrapping in `Ractor::RemoteError` (needs nested-class NAMES and `.cause`
//! chaining, both pre-existing documented gaps); an UNSHAREABLE final value
//! is a loud panic at join (the common failure -- an unshareable `#send`
//! payload -- is the catchable error); `Ractor.receive` works only inside a
//! spawned Ractor (the main Ractor has no incoming port here); errors use
//! the flat prelude class `RactorError` standing in for `Ractor::Error`.

use crate::{RubyValue, Signal};
use parking_lot::Mutex as PlMutex;
use std::cell::RefCell;
use std::sync::mpsc;
use std::sync::Arc;

enum RactorState {
    Running(std::thread::JoinHandle<Result<RubyValue, Signal>>),
    Done(Result<RubyValue, Signal>),
}

pub struct RactorData {
    incoming: mpsc::Sender<RubyValue>,
    state: PlMutex<Option<RactorState>>,
}

pub type RRactor = Arc<RactorData>;

thread_local! {
    /// The receiving end of THIS thread's Ractor's incoming port --
    /// installed at spawn, `None` on every other thread (including main).
    static CURRENT_INCOMING: RefCell<Option<mpsc::Receiver<RubyValue>>> = const { RefCell::new(None) };
}

/// `Ractor.new(*args) { |*params| ... }` -- `args` cross the boundary NOW
/// (CRuby sends them through the ordinary message path). `Err` = an
/// unshareable/uncopyable arg's message, raised as `RactorError` by codegen.
pub fn ractor_new(block: RubyValue, args: Vec<RubyValue>) -> Result<RubyValue, String> {
    let body = block.as_proc_unchecked();
    let crossed: Vec<RubyValue> =
        args.iter().map(cross_boundary).collect::<Result<_, _>>()?;
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        CURRENT_INCOMING.with(|c| *c.borrow_mut() = Some(rx));
        body(&crossed)
    });
    Ok(RubyValue::Ractor(Arc::new(RactorData {
        incoming: tx,
        state: PlMutex::new(Some(RactorState::Running(handle))),
    })))
}

/// `Ractor#send(obj)` -- the payload crosses the boundary (by reference if
/// shareable, deep copy otherwise); `Err` carries the message for codegen's
/// `RactorError`.
pub fn ractor_send(r: &RRactor, value: &RubyValue) -> Result<(), String> {
    let crossed = cross_boundary(value)?;
    r.incoming
        .send(crossed)
        .map_err(|_| "The incoming-port is already closed".to_string())
}

/// `Ractor.receive` -- blocks THIS Ractor's OS thread until a message
/// arrives. Loud panics (not catchable errors) for the two spike-scope
/// walls: calling it outside a spawned Ractor, and every sender handle
/// being gone while blocked (a real deadlock either way).
pub fn ractor_receive() -> RubyValue {
    CURRENT_INCOMING.with(|c| {
        let slot = c.borrow();
        let Some(rx) = slot.as_ref() else {
            panic!("Ractor.receive outside a spawned Ractor isn't supported yet (spike scope -- the main Ractor has no incoming port)");
        };
        rx.recv()
            .expect("every handle to this Ractor was dropped while it blocked in Ractor.receive")
    })
}

/// `#value`/`#join`'s shared wait -- joins the OS thread once, crosses the
/// final value out (loud panic if unshareable -- see module docs), caches
/// the outcome so repeat calls agree, and re-raises an uncaught `Signal` in
/// the CALLER (the `Thread` precedent; the `Ractor::RemoteError` wrapper is
/// a documented skip).
pub fn ractor_outcome(r: &RRactor) -> Result<RubyValue, Signal> {
    let taken = r.state.lock().take();
    match taken {
        Some(RactorState::Running(handle)) => {
            let outcome = match handle.join() {
                Ok(Ok(v)) => match cross_boundary(&v) {
                    Ok(crossed) => Ok(crossed),
                    Err(msg) => panic!("this Ractor's final value can't cross back: {msg}"),
                },
                Ok(Err(sig)) => Err(sig),
                Err(panic_payload) => std::panic::resume_unwind(panic_payload),
            };
            *r.state.lock() = Some(RactorState::Done(outcome.clone()));
            outcome
        }
        Some(RactorState::Done(outcome)) => {
            *r.state.lock() = Some(RactorState::Done(outcome.clone()));
            outcome
        }
        None => panic!("concurrent value/join on the same Ractor isn't supported yet (spike scope)"),
    }
}

/// `Ractor.shareable?` -- the recursive predicate (CRuby's rule: frozen AND
/// everything reachable shareable; immediates/Symbols/Ractors inherently
/// shareable; `Regexp` immutable here so always shareable). Cycle-guarded
/// (Phase 15.2) via the same visited-set mechanism as `inspect_string`'s
/// (`value::container_identity`): a container already under examination
/// contributes `true` at its re-entry point -- the cycle itself never makes
/// a graph unshareable, only an unfrozen/unshareable NODE does, and every
/// node is still visited exactly once.
pub fn shareable(v: &RubyValue) -> bool {
    shareable_guarded(v, &mut Vec::new())
}

fn shareable_guarded(v: &RubyValue, seen: &mut Vec<usize>) -> bool {
    if let Some(ptr) = crate::value::container_identity(v) {
        if seen.contains(&ptr) {
            return true;
        }
        seen.push(ptr);
    }
    match v {
        RubyValue::Nil
        | RubyValue::Bool(_)
        | RubyValue::Int(_)
        | RubyValue::Float(_)
        | RubyValue::Symbol(_)
        | RubyValue::Regexp(_)
        | RubyValue::Ractor(_)
        // A class handle is inherently shareable (Phase 16.1): classes are
        // process-wide in real Ruby too.
        | RubyValue::Class(_) => true,
        RubyValue::Range(start, end, _) => {
            start.as_deref().is_none_or(|s| shareable_guarded(s, seen))
                && end.as_deref().is_none_or(|e| shareable_guarded(e, seen))
        }
        RubyValue::Str(s) => s.is_frozen(),
        RubyValue::Array(a) => a.is_frozen() && a.lock().iter().all(|e| shareable_guarded(e, seen)),
        RubyValue::Hash(h) => {
            h.is_frozen()
                && h.lock()
                    .values()
                    .all(|(k, val)| shareable_guarded(k, seen) && shareable_guarded(val, seen))
        }
        RubyValue::Object(o) => {
            o.is_frozen() && o.ivar_values().iter().all(|iv| shareable_guarded(iv, seen))
        }
        RubyValue::Proc(_)
        | RubyValue::Fiber(_)
        | RubyValue::Thread(_)
        | RubyValue::Mutex(_)
        | RubyValue::Queue(_)
        | RubyValue::MatchData(_) => false,
    }
}

/// `Ractor.make_shareable(obj)` -- the deep-freeze traversal (CRuby's
/// `rb_ractor_make_shareable`: walk the reachable subgraph, freeze each
/// node). Returns the (now shareable) value itself; `Err` when the graph
/// contains something that can never be shareable. Cycle-guarded (Phase
/// 15.2, retiring the documented stack-overflow gap): an already-visited
/// container is already frozen-and-being-walked, so its re-entry is a
/// no-op. Note `seen` here is a PERMANENT visited set, not `display_with`'s
/// pop-on-exit stack -- freezing is idempotent and each node needs walking
/// only once, and (unlike printing) there's no output that would differ.
pub fn make_shareable(v: &RubyValue) -> Result<RubyValue, String> {
    make_shareable_guarded(v, &mut Vec::new())?;
    Ok(v.clone())
}

fn make_shareable_guarded(v: &RubyValue, seen: &mut Vec<usize>) -> Result<(), String> {
    if let Some(ptr) = crate::value::container_identity(v) {
        if seen.contains(&ptr) {
            return Ok(());
        }
        seen.push(ptr);
    }
    match v {
        RubyValue::Nil
        | RubyValue::Bool(_)
        | RubyValue::Int(_)
        | RubyValue::Float(_)
        | RubyValue::Symbol(_)
        | RubyValue::Regexp(_)
        | RubyValue::Ractor(_)
        | RubyValue::Class(_) => {}
        RubyValue::Range(start, end, _) => {
            if let Some(s) = start.as_deref() {
                make_shareable_guarded(s, seen)?;
            }
            if let Some(e) = end.as_deref() {
                make_shareable_guarded(e, seen)?;
            }
        }
        RubyValue::Str(s) => s.set_frozen(),
        RubyValue::Array(a) => {
            a.set_frozen();
            for elem in a.lock().iter() {
                make_shareable_guarded(elem, seen)?;
            }
        }
        RubyValue::Hash(h) => {
            h.set_frozen();
            for (k, val) in h.lock().values() {
                make_shareable_guarded(k, seen)?;
                make_shareable_guarded(val, seen)?;
            }
        }
        RubyValue::Object(o) => {
            o.set_frozen();
            for iv in o.ivar_values() {
                make_shareable_guarded(&iv, seen)?;
            }
        }
        other => {
            return Err(format!(
                "can't make shareable object: {}",
                other.to_display_string()
            ))
        }
    }
    Ok(())
}

/// By reference when shareable, deep copy otherwise -- see module docs for
/// the unfrozen-Object rejection.
fn cross_boundary(v: &RubyValue) -> Result<RubyValue, String> {
    if shareable(v) {
        return Ok(v.clone());
    }
    match v {
        RubyValue::Str(s) => Ok(RubyValue::Str(crate::string_new(s.lock().clone()))),
        RubyValue::Array(a) => {
            let copied: Vec<RubyValue> =
                a.lock().iter().map(cross_boundary).collect::<Result<_, _>>()?;
            Ok(RubyValue::Array(crate::array_new(copied)))
        }
        RubyValue::Hash(h) => {
            let pairs: Vec<(RubyValue, RubyValue)> = h
                .lock()
                .values()
                .map(|(k, val)| Ok((cross_boundary(k)?, cross_boundary(val)?)))
                .collect::<Result<_, String>>()?;
            Ok(RubyValue::Hash(crate::hash_new(pairs)))
        }
        RubyValue::Range(start, end, excl) => {
            let cross_end = |e: &Option<Box<RubyValue>>| -> Result<_, String> {
                Ok(match e.as_deref() {
                    Some(v) => Some(Box::new(cross_boundary(v)?)),
                    None => None,
                })
            };
            Ok(RubyValue::Range(cross_end(start)?, cross_end(end)?, *excl))
        }
        RubyValue::Object(_) => Err(
            "an unfrozen Object can't cross a Ractor boundary (spike scope: real Ruby deep-copies it; here, Ractor.make_shareable it first)"
                .to_string(),
        ),
        other => Err(format!(
            "{} can't cross a Ractor boundary",
            other.to_display_string()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shareability_tiering_and_deep_copy() {
        assert!(shareable(&RubyValue::Int(1)));
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(!shareable(&s));
        make_shareable(&s).unwrap();
        assert!(shareable(&s));

        // Deep copy: mutating the original after crossing leaves the copy
        // untouched.
        let arr = RubyValue::Array(crate::array_new(vec![RubyValue::Int(1)]));
        let crossed = cross_boundary(&arr).unwrap();
        crate::array_set(&arr.as_array_unchecked(), 0, RubyValue::Int(99));
        assert_eq!(crossed.as_array_unchecked().lock()[0].to_display_string(), "1");

        // A deeply-frozen array crosses by REFERENCE (same storage).
        let frozen = RubyValue::Array(crate::array_new(vec![RubyValue::Int(2)]));
        make_shareable(&frozen).unwrap();
        let shared = cross_boundary(&frozen).unwrap();
        assert!(Arc::ptr_eq(
            &frozen.as_array_unchecked(),
            &shared.as_array_unchecked()
        ));

        assert!(make_shareable(&RubyValue::Proc(Arc::new(|_| Ok(RubyValue::Nil)))).is_err());
    }

    /// Phase 15.2 cycle guards: a self-referential graph used to recurse to
    /// stack overflow in both traversals; now every node is visited once.
    #[test]
    fn make_shareable_handles_a_self_referential_array() {
        let arr = crate::array_new(vec![RubyValue::Int(1)]);
        crate::array_push(&arr, RubyValue::Array(arr.clone()));
        let v = RubyValue::Array(arr.clone());

        make_shareable(&v).unwrap();
        assert!(arr.is_frozen());
        assert!(shareable(&v), "a frozen cycle is shareable (every node frozen)");
    }

    #[test]
    fn make_shareable_handles_a_self_referential_hash() {
        let h = crate::hash_new(vec![(
            RubyValue::Symbol(crate::Symbol::intern("k")),
            RubyValue::Int(1),
        )]);
        crate::hash_set(
            &h,
            RubyValue::Symbol(crate::Symbol::intern("me")),
            RubyValue::Hash(h.clone()),
        );
        let v = RubyValue::Hash(h.clone());

        make_shareable(&v).unwrap();
        assert!(h.is_frozen());
        assert!(shareable(&v));
    }

    /// The predicate must also TERMINATE (not just avoid wrong answers) on
    /// an unfrozen cycle -- and answer false, since the nodes are unfrozen.
    #[test]
    fn shareable_terminates_and_rejects_an_unfrozen_cycle() {
        let arr = crate::array_new(vec![RubyValue::Int(1)]);
        crate::array_push(&arr, RubyValue::Array(arr.clone()));

        assert!(!shareable(&RubyValue::Array(arr)));
    }

    /// A cycle that runs THROUGH two containers (array -> hash -> array),
    /// not just direct self-reference.
    #[test]
    fn make_shareable_handles_a_cross_container_cycle() {
        let arr = crate::array_new(vec![RubyValue::Int(1)]);
        let h = crate::hash_new(vec![(
            RubyValue::Symbol(crate::Symbol::intern("back")),
            RubyValue::Array(arr.clone()),
        )]);
        crate::array_push(&arr, RubyValue::Hash(h.clone()));
        let v = RubyValue::Array(arr.clone());

        make_shareable(&v).unwrap();
        assert!(arr.is_frozen());
        assert!(h.is_frozen());
        assert!(shareable(&v));
    }
}
