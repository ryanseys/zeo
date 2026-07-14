//! spinel-rt: the runtime library every spinelc-generated program links
//! against. Mirrors spinel's `lib/` (the C runtime), scoped to exactly what
//! the spike's 7 examples need. See `~/dev/spinel-rs/docs/PORTING_ANALYSIS.md`
//! for the full design writeup.

mod arith;
mod collections;
mod cvars;
mod dispatch;
mod rproc;
mod signal;
mod symbol;
mod value;

pub use arith::*;
pub use collections::*;
pub use dispatch::{
    downcast_robj, install_class_registry, is_a, send, ClassId, ClassRegistry, MethodFn, Object,
    RObj, RubyObject,
};
pub use cvars::{cvar_get, cvar_set};
pub use rproc::RProc;
pub use signal::{catch_break, Signal};
pub use symbol::Symbol;
pub use value::RubyValue;

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
/// interior-mutable field (`RefCell<RubyValue>`) so every generated method
/// can uniformly take a receiver (see "A named risk" in the plan).
///
/// Every method receiver is `self: Rc<Self>`, not `&self` -- Phase 6 needs
/// this: a real escaping `Proc` closure that references `self`/an ivar has
/// to capture an OWNED, `'static` handle (a `RubyValue`-stored `Proc` has no
/// lifetime parameter anywhere in this codebase), which a borrowed `&self`
/// can never provide. Every call site already hands over an `Rc<Concrete>`
/// (`New`'s result, and every local/ivar holding an object, are `Rc`-wrapped
/// from the moment they're built -- see `new_handle`'s docs below), so this
/// costs nothing at ordinary call sites; it only removes a capability
/// (capturing `self` into a closure) that didn't exist before.
///
/// `$slf` (not a hardcoded `self`) is still captured from the caller's own
/// tokens, exactly as before the `Rc<Self>` migration -- confirmed the hard
/// way: a `self` written directly in THIS template is hygienically distinct
/// from a `self` written inside the caller's `$body`, so `rustc` rejects it
/// (`E0424`, "self value is a keyword only available in methods with a self
/// parameter") the moment `$body` references `self.some_ivar`. Only the
/// TYPE annotation is now fixed to `Rc<Self>` (rather than varying, since
/// `&self` never varied either -- `$slf:tt` only ever captured the single
/// token `self`, never its type).
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
            $( def $method:ident ( $slf:tt : std::rc::Rc<Self> $(, $arg:ident : $arg_ty:ty)* $(,)? ) $body:block )*
            dispatch { $( $dname:ident => $tramp:expr ),* $(,)? }
        }
    ) => {
        pub struct $name {
            $( pub $ivar: std::cell::RefCell<$crate::RubyValue>, )*
        }

        impl $name {
            pub const CLASS_ID: $crate::ClassId = $crate::ClassId($id);

            /// Accepts an already-`Rc`-wrapped value (never `Self` by value):
            /// generated code stores every `New`-constructed object as
            /// `Rc<ConcreteStruct>` from the moment it's built (see
            /// `codegen::call::emit_new`), so that a local variable holding
            /// it can be read more than once via a cheap, identity-preserving
            /// `Rc::clone()` rather than needing (and not having) a `Clone`
            /// impl on the bare struct itself -- deriving one naively would
            /// deep-copy each `RefCell` ivar, breaking Ruby's shared-mutable-
            /// object-identity semantics (`b = Box.new(1); c = b` must alias,
            /// not duplicate). This just returns `inner` unchanged, relying on
            /// `Rc<Concrete> -> Rc<dyn RubyObject>` unsized coercion at the
            /// return site to produce `RObj`.
            pub fn new_handle(inner: std::rc::Rc<Self>) -> $crate::RObj {
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
                pub fn $method($slf: std::rc::Rc<Self> $(, $arg: $arg_ty)*) -> Result<$crate::RubyValue, $crate::Signal> $body
            )*
        }

        impl $crate::RubyObject for $name {
            fn class_id(&self) -> $crate::ClassId { Self::CLASS_ID }
            fn as_any(&self) -> &dyn std::any::Any { self }
            fn as_any_rc(self: std::rc::Rc<Self>) -> std::rc::Rc<dyn std::any::Any> { self }
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
            pub fn __register(registry: &mut $crate::ClassRegistry) {
                registry.register(Self::CLASS_ID, vec![$($crate::ClassId($anc)),*]);
                $(
                    registry.define_method(
                        Self::CLASS_ID,
                        $crate::Symbol::intern(stringify!($dname)),
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
            def initialize(self: std::rc::Rc<Self>, x: RubyValue) { *self.x.borrow_mut() = x; Ok(RubyValue::Nil) }
            def x(self: std::rc::Rc<Self>) { Ok(self.x.borrow().clone()) }
            dispatch {
                initialize => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Point>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [x] => Point::initialize(this, x.clone()),
                        _ => panic!("wrong number of arguments for initialize"),
                    }
                },
                x => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
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
            def hello(self: std::rc::Rc<Self>) { Ok(RubyValue::Str(string_new("hi".to_string()))) }
            def method_missing(self: std::rc::Rc<Self>, name: RubyValue) {
                Ok(RubyValue::Str(string_new(format!("no such method: {}", name.to_display_string()))))
            }
            dispatch {
                hello => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Greeter>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [] => Greeter::hello(this),
                        _ => panic!("wrong number of arguments for hello"),
                    }
                },
                method_missing => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Greeter>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [name] => Greeter::method_missing(this, name.clone()),
                        _ => panic!("wrong number of arguments for method_missing"),
                    }
                },
            }
        }
    }

    fn install() {
        let mut registry = ClassRegistry::new();
        registry.register(Object::CLASS_ID, vec![Object::CLASS_ID]);
        Point::__register(&mut registry);
        Greeter::__register(&mut registry);
        install_class_registry(registry);
    }

    #[test]
    fn static_path_stores_and_reads_ivar() {
        let p = std::rc::Rc::new(Point {
            x: std::cell::RefCell::new(RubyValue::Nil),
        });
        p.clone().initialize(RubyValue::Int(5)).unwrap();
        match p.x().unwrap() {
            RubyValue::Int(5) => {}
            other => panic!("expected Int(5), got {}", other.to_display_string()),
        }
    }

    #[test]
    fn new_handle_erases_to_a_trait_object() {
        let handle: RObj = Point::new_handle(std::rc::Rc::new(Point {
            x: std::cell::RefCell::new(RubyValue::Int(7)),
        }));
        assert_eq!(handle.class_id(), Point::CLASS_ID);
    }

    #[test]
    fn dynamic_send_finds_registered_method() {
        install();
        let g: RObj = Greeter::new_handle(std::rc::Rc::new(Greeter {}));
        let result = send(&g, Symbol::intern("hello"), &[], None).unwrap();
        assert_eq!(result.to_display_string(), "hi");
    }

    #[test]
    fn dynamic_send_falls_back_to_method_missing() {
        install();
        let g: RObj = Greeter::new_handle(std::rc::Rc::new(Greeter {}));
        let result = send(&g, Symbol::intern("nope"), &[], None).unwrap();
        assert_eq!(result.to_display_string(), "no such method: nope");
    }
}
