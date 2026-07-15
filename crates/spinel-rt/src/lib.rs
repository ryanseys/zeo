//! spinel-rt: the runtime library every spinelc-generated program links
//! against. Mirrors spinel's `lib/` (the C runtime), scoped to exactly what
//! the spike's 7 examples need. See `~/dev/spinel-rs/docs/PORTING_ANALYSIS.md`
//! for the full design writeup.

mod arith;
mod collections;
mod constants;
mod cvars;
mod dispatch;
mod exec;
mod fiber;
mod globals;
mod handling;
mod ractor;
mod regexp;
mod rproc;
mod signal;
mod symbol;
mod thread;
mod value;

pub use arith::*;
pub use collections::*;
pub use constants::{const_get, const_set};
pub use dispatch::{
    downcast_robj, install_class_registry, install_no_method_error_factory, is_a, responds_to,
    send, ClassId, ClassRegistry,
    MethodFn, Object, RObj, RubyObject, ARRAY_CLASS, FALSE_CLASS, FIBER_CLASS, FLOAT_CLASS,
    HASH_CLASS, INTEGER_CLASS, MATCH_DATA_CLASS, MUTEX_CLASS, NIL_CLASS, PROC_CLASS,
    QUEUE_CLASS, RACTOR_CLASS, RANGE_CLASS, REGEXP_CLASS, STRING_CLASS, SYMBOL_CLASS,
    THREAD_CLASS, TRUE_CLASS,
};
pub use cvars::{cvar_get, cvar_set};
pub use exec::run_main;
pub use fiber::{fiber_alive, fiber_new, fiber_resume, fiber_yield, FiberHandle, FiberResume, RFiber};
pub use globals::{global_get, global_set};
pub use handling::{current_exception, pop_handling, push_handling};
pub use ractor::{
    make_shareable, ractor_new, ractor_outcome, ractor_receive, ractor_send, shareable,
    RRactor, RactorData,
};
pub use regexp::*;
pub use rproc::RProc;
pub use signal::{catch_break, Signal};
pub use symbol::Symbol;
pub use thread::{
    mutex_lock, mutex_locked, mutex_new, mutex_owned, mutex_unlock, queue_close, queue_closed,
    queue_len, queue_new, queue_pop, queue_push, thread_new, thread_outcome, MutexData,
    QueueData, RMutex, RQueue, RThread, ThreadData,
};
pub use value::RubyValue;

/// Re-exported so `ruby_class!`'s macro-expanded code (which runs inside a
/// GENERATED program's own crate, not this one) can reference
/// `$crate::parking_lot::Mutex` without that program's own `Cargo.toml`
/// needing a direct `parking_lot` dependency -- `parking_lot` stays an
/// implementation detail of this runtime crate (Part 9).
pub use parking_lot;

/// A thin, poison-free wrapper around `parking_lot::Mutex::lock` -- a single
/// choke point every generated read/write site goes through (Part 9), in
/// case a uniform policy ever needs one later. `parking_lot`'s `Mutex` has
/// no poisoning at all (unlike `std::sync::Mutex`), matching Ruby's own
/// semantics: there's no such thing as a "poisoned object" if a thread
/// panics while holding a lock on it.
pub fn lock<T>(m: &parking_lot::Mutex<T>) -> parking_lot::MutexGuard<'_, T> {
    m.lock()
}

/// Mirrors CRuby's `Kernel#puts` for the single scalar-argument case (the
/// only form the spike's examples use): print the value, adding a trailing
/// newline only if it doesn't already end in one.
pub fn puts(value: RubyValue) {
    let s = value.to_display_string();
    if s.ends_with('\n') {
        print!("{s}");
    } else {
        println!("{s}");
    }
}

/// One declarative macro absorbs the struct/trait-impl/registration ceremony
/// spinel's codegen hand-emits as raw C text per class (`emit_class_struct`,
/// `emit_class_new`, etc.) -- see the plan's "The `ruby_class!` macro"
/// section for the full rationale. Each ivar becomes an individually
/// interior-mutable field (`parking_lot::Mutex<RubyValue>`, not `RefCell` --
/// see the plan's Part 9) so every generated method can uniformly take a
/// receiver (see "A named risk" in the plan), and so every generated struct
/// is genuinely `Send + Sync` for `Thread`/`Ractor` to eventually use.
///
/// Every method receiver is `self: Arc<Self>`, not `&self` -- Phase 6 needs
/// this: a real escaping `Proc` closure that references `self`/an ivar has
/// to capture an OWNED, `'static` handle (a `RubyValue`-stored `Proc` has no
/// lifetime parameter anywhere in this codebase), which a borrowed `&self`
/// can never provide. Every call site already hands over an `Arc<Concrete>`
/// (`New`'s result, and every local/ivar holding an object, are `Arc`-wrapped
/// from the moment they're built -- see `new_handle`'s docs below), so this
/// costs nothing at ordinary call sites; it only removes a capability
/// (capturing `self` into a closure) that didn't exist before.
///
/// `$slf` (not a hardcoded `self`) is still captured from the caller's own
/// tokens, exactly as before the `Rc<Self>` -> `Arc<Self>` migration --
/// confirmed the hard way: a `self` written directly in THIS template is
/// hygienically distinct from a `self` written inside the caller's `$body`,
/// so `rustc` rejects it (`E0424`, "self value is a keyword only available
/// in methods with a self parameter") the moment `$body` references
/// `self.some_ivar`. Only the TYPE annotation is now fixed to `Arc<Self>`
/// (rather than varying, since `&self` never varied either -- `$slf:tt`
/// only ever captured the single token `self`, never its type).
///
/// `ancestors` is the full, real linearized MRO (this class first, then
/// prepends/includes/superclass in resolution order -- see
/// `spinelc::analyze::mro::compute_ancestors`), computed entirely at
/// spinelc compile time and baked in here as a literal list; `__register`
/// just forwards it. No separate `ruby_module!` macro exists: a Ruby
/// `module`'s methods are never emitted as their own Rust struct/impl at
/// all -- they're MATERIALIZED directly onto whichever class(es)
/// include/prepend/extend them (see the plan's Part 6), so every method
/// this macro ever sees already belongs, concretely, to `$name`.
#[macro_export]
macro_rules! ruby_class {
    (
        class $name:ident : $super:path {
            id: $id:expr;
            ancestors: [ $($anc:expr),* $(,)? ];
            ivars { $($ivar:ident),* $(,)? }
            $( def $method:ident ( $slf:tt : std::sync::Arc<Self> $(, $arg:ident : $arg_ty:ty)* $(,)? ) $body:block )*
            dispatch { $( $dname:literal => $tramp:expr ),* $(,)? }
        }
    ) => {
        pub struct $name {
            /// `.freeze`'s per-object flag (Phase 13.1) -- read through
            /// `RubyObject::is_frozen` and by the guard codegen emits before
            /// every ivar write (`emit_ivar_write_stmt`). Double-underscore
            /// prefixed, matching the `__blk`/`__self` convention for
            /// generated names a user ivar won't realistically collide with
            /// (a literal Ruby `@__frozen` would -- same accepted, vanishing
            /// residual risk as every other `__`-reserved name in codegen).
            pub __frozen: std::sync::atomic::AtomicBool,
            $( pub $ivar: $crate::parking_lot::Mutex<$crate::RubyValue>, )*
        }

        impl $name {
            pub const CLASS_ID: $crate::ClassId = $crate::ClassId($id);

            /// Accepts an already-`Arc`-wrapped value (never `Self` by
            /// value): generated code stores every `New`-constructed object
            /// as `Arc<ConcreteStruct>` from the moment it's built (see
            /// `codegen::call::emit_new`), so that a local variable holding
            /// it can be read more than once via a cheap, identity-preserving
            /// `Arc::clone()` rather than needing (and not having) a `Clone`
            /// impl on the bare struct itself -- deriving one naively would
            /// deep-copy each `Mutex` ivar, breaking Ruby's shared-mutable-
            /// object-identity semantics (`b = Box.new(1); c = b` must alias,
            /// not duplicate). This just returns `inner` unchanged, relying on
            /// `Arc<Concrete> -> Arc<dyn RubyObject>` unsized coercion at the
            /// return site to produce `RObj`. `Arc` (not `Rc`, Part 9): every
            /// generated struct is genuinely `Send + Sync` once its fields
            /// are `Mutex`-wrapped, needed for `Thread`/`Ractor` to ever cross
            /// a real OS thread boundary -- no `unsafe impl` involved, this
            /// falls out of ordinary auto-trait derivation.
            pub fn new_handle(inner: std::sync::Arc<Self>) -> $crate::RObj {
                inner
            }

            $(
                // Return type is always `Result<RubyValue, Signal>`, never
                // omitted or bare `RubyValue`: Ruby methods always implicitly
                // return a value (the last expression, or nil) -- codegen
                // emits `Ok(RubyValue::Nil)` for methods with nothing
                // meaningful to return (e.g. `initialize`) -- and every
                // method body is a `?`-propagation boundary for non-local
                // control flow from the first breadth phase onward, not just
                // once `raise`/escaping `Proc`s exist (see `signal.rs`).
                pub fn $method($slf: std::sync::Arc<Self> $(, $arg: $arg_ty)*) -> Result<$crate::RubyValue, $crate::Signal> $body
            )*
        }

        impl $crate::RubyObject for $name {
            fn class_id(&self) -> $crate::ClassId { Self::CLASS_ID }
            fn as_any(&self) -> &dyn std::any::Any { self }
            fn as_any_rc(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync> { self }
            fn is_frozen(&self) -> bool { self.__frozen.load(std::sync::atomic::Ordering::Relaxed) }
            fn set_frozen(&self) { self.__frozen.store(true, std::sync::atomic::Ordering::Relaxed) }
            fn ivar_values(&self) -> Vec<$crate::RubyValue> {
                vec![ $( self.$ivar.lock().clone() ),* ]
            }
        }

        impl $name {
            /// Called once from generated `main()`. Populates the *runtime*
            /// dispatch table (Path 2) with a trampoline per method; Path 1
            /// (static) call sites never go through this table at all.
            ///
            /// Each trampoline's BODY is supplied by the caller (`$tramp`,
            /// via the `dispatch { ... }` block below) rather than derived
            /// here from `$arg`/`$arg_ty` pairs: once a method's params can
            /// be optional/rest/keyword instead of uniformly required, a
            /// bare exact-length slice-pattern match (the ONLY thing this
            /// macro could derive on its own from the `def` clauses above)
            /// can no longer express the binding logic -- spinelc already
            /// has the full, precise parameter-kind info to author it
            /// correctly (see `codegen::params`), so this macro's job
            /// shrinks to exactly what its own doc comment always claimed:
            /// struct/impl/registration ceremony, not binding logic.
            ///
            /// `$dname` is a STRING LITERAL of the method's real Ruby name
            /// (`"tag="`, `"empty?"`, ...), not the escaped Rust identifier
            /// used for the `def` clause above (`tag_set`, `empty_p`, ...) --
            /// confirmed the hard way this distinction matters: an earlier
            /// version used `$dname:ident` + `stringify!($dname)` here, which
            /// registered every escaped-name method under its ESCAPED name
            /// instead of its real one, silently breaking `send`/`rescue`'s
            /// own re-raise-then-`.send(:tag=, ...)` for any method whose
            /// name needed escaping at all.
            pub fn __register(registry: &mut $crate::ClassRegistry) {
                registry.register(Self::CLASS_ID, vec![$($crate::ClassId($anc)),*]);
                $(
                    registry.define_method(
                        Self::CLASS_ID,
                        $crate::Symbol::intern($dname),
                        $tramp,
                    );
                )*
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    ruby_class! {
        class Point : Object {
            id: 1;
            ancestors: [1, 0];
            ivars { x }
            def initialize(self: std::sync::Arc<Self>, x: RubyValue) { *self.x.lock() = x; Ok(RubyValue::Nil) }
            def x(self: std::sync::Arc<Self>) { Ok(self.x.lock().clone()) }
            dispatch {
                "initialize" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Point>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [x] => Point::initialize(this, x.clone()),
                        _ => panic!("wrong number of arguments for initialize"),
                    }
                },
                "x" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Point>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [] => Point::x(this),
                        _ => panic!("wrong number of arguments for x"),
                    }
                },
            }
        }
    }

    ruby_class! {
        class Greeter : Object {
            id: 2;
            ancestors: [2, 0];
            ivars { }
            def hello(self: std::sync::Arc<Self>) { Ok(RubyValue::Str(string_new("hi".to_string()))) }
            def method_missing(self: std::sync::Arc<Self>, name: RubyValue) {
                Ok(RubyValue::Str(string_new(format!("no such method: {}", name.to_display_string()))))
            }
            dispatch {
                "hello" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Greeter>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [] => Greeter::hello(this),
                        _ => panic!("wrong number of arguments for hello"),
                    }
                },
                "method_missing" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Greeter>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [name] => Greeter::method_missing(this, name.clone()),
                        _ => panic!("wrong number of arguments for method_missing"),
                    }
                },
            }
        }
    }

    /// The class registry is now a genuinely process-wide `OnceLock` (Part
    /// 9), correctly rejecting a second install -- exactly the "install
    /// once, from `main()`, before anything else runs" contract a real
    /// generated program relies on. Rust's test harness runs each `#[test]`
    /// on its own OS thread, so more than one test in this module calling
    /// `install()` would previously "work" only because each thread's own
    /// `thread_local!` gave it an independent (and therefore untested-
    /// against-each-other) copy -- `std::sync::Once` makes the test helper
    /// itself match the real one-time-installation contract instead.
    fn install() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let mut registry = ClassRegistry::new();
            registry.register(Object::CLASS_ID, vec![Object::CLASS_ID]);
            Point::__register(&mut registry);
            Greeter::__register(&mut registry);
            install_class_registry(registry);
        });
    }

    #[test]
    fn static_path_stores_and_reads_ivar() {
        let p = std::sync::Arc::new(Point {
            __frozen: Default::default(),
            x: parking_lot::Mutex::new(RubyValue::Nil),
        });
        p.clone().initialize(RubyValue::Int(5)).unwrap();
        match p.x().unwrap() {
            RubyValue::Int(5) => {}
            other => panic!("expected Int(5), got {}", other.to_display_string()),
        }
    }

    #[test]
    fn new_handle_erases_to_a_trait_object() {
        let handle: RObj = Point::new_handle(std::sync::Arc::new(Point {
            __frozen: Default::default(),
            x: parking_lot::Mutex::new(RubyValue::Int(7)),
        }));
        assert_eq!(handle.class_id(), Point::CLASS_ID);
    }

    /// Phase 13.1: the freeze tiering `RubyValue::is_frozen`/`freeze_value`
    /// implement -- immediates/`Range` always frozen, mutable types start
    /// unfrozen and latch on `.freeze` (which returns self and no-ops when
    /// repeated), the flagless `Proc` approximation stays `false`.
    #[test]
    fn freeze_tiering_matches_cruby_semantics() {
        assert!(RubyValue::Int(1).is_frozen());
        assert!(RubyValue::Nil.is_frozen());
        assert!(RubyValue::Symbol(Symbol::intern("s")).is_frozen());
        assert!(RubyValue::Range(None, None, false).is_frozen());

        let arr = RubyValue::Array(array_new(vec![RubyValue::Int(1)]));
        assert!(!arr.is_frozen());
        let same = arr.freeze_value();
        assert!(arr.is_frozen());
        // Returns SELF (the same shared storage), not a copy.
        assert!(same.is_frozen());
        arr.freeze_value(); // repeat freeze is a silent no-op
        assert!(arr.is_frozen());

        let s = RubyValue::Str(string_new("abc".to_string()));
        assert!(!s.is_frozen());
        s.freeze_value();
        assert!(s.is_frozen());

        let obj = RubyValue::Object(Point::new_handle(std::sync::Arc::new(Point {
            __frozen: Default::default(),
            x: parking_lot::Mutex::new(RubyValue::Nil),
        })));
        assert!(!obj.is_frozen());
        obj.freeze_value();
        assert!(obj.is_frozen());

        assert_eq!(
            arr.inspect_string(),
            "[1]",
            "inspect backs the FrozenError message format"
        );
    }

    #[test]
    fn dynamic_send_finds_registered_method() {
        install();
        let g: RObj = Greeter::new_handle(std::sync::Arc::new(Greeter { __frozen: Default::default() }));
        let result = send(&g, Symbol::intern("hello"), &[], None).unwrap();
        assert_eq!(result.to_display_string(), "hi");
    }

    #[test]
    fn dynamic_send_falls_back_to_method_missing() {
        install();
        let g: RObj = Greeter::new_handle(std::sync::Arc::new(Greeter { __frozen: Default::default() }));
        let result = send(&g, Symbol::intern("nope"), &[], None).unwrap();
        assert_eq!(result.to_display_string(), "no such method: nope");
    }

    /// Part 9 (Send+Sync migration) regression guard: fails to compile if
    /// `RubyValue`, `Signal`, or a generated class ever regains an `Rc`/
    /// `RefCell` anywhere in its type graph -- catches the mistake via the
    /// type system immediately, rather than silently reintroducing a
    /// `!Send`/`!Sync` blocker.
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn core_types_are_send_and_sync() {
        assert_send_sync::<RubyValue>();
        assert_send_sync::<Signal>();
        assert_send_sync::<Point>();
        assert_send_sync::<Greeter>();
        assert_send_sync::<RObj>();
        assert_send_sync::<RProc>();
    }
}
