//! spinel-rt: the runtime library every spinelc-generated program links
//! against. Mirrors spinel's `lib/` (the C runtime), scoped to exactly what
//! the spike's 7 examples need. See `~/dev/spinel-rs/docs/PORTING_ANALYSIS.md`
//! for the full design writeup.

mod dispatch;
mod symbol;
mod value;

pub use dispatch::{
    install_class_registry, send, ClassId, ClassRegistry, MethodFn, Object, RObj, RubyObject,
};
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

/// Mirrors `sp_int_add` (the default `--int-overflow=raise` mode,
/// `lib/sp_runtime.h:140-242`): checked addition, raising on overflow rather
/// than silently wrapping. Spinel raises a catchable `RangeError`; the spike
/// has no `raise`/`rescue` yet (see the plan's Phase 3), so this panics --
/// still fail-fast, not silent-wrong.
pub fn int_add(a: i64, b: i64) -> i64 {
    a.checked_add(b).expect("Integer overflow")
}

/// One declarative macro absorbs the struct/trait-impl/registration ceremony
/// spinel's codegen hand-emits as raw C text per class (`emit_class_struct`,
/// `emit_class_new`, etc.) -- see the plan's "The `ruby_class!` macro"
/// section for the full rationale. Each ivar becomes an individually
/// interior-mutable field (`RefCell<RubyValue>`) so every generated method
/// can uniformly take `&self` (see "A named risk" in the plan).
#[macro_export]
macro_rules! ruby_class {
    (
        class $name:ident : $super:path {
            id: $id:expr;
            ivars { $($ivar:ident),* $(,)? }
            $( def $method:ident ( & $slf:tt $(, $arg:ident : $arg_ty:ty)* $(,)? ) $body:block )*
        }
    ) => {
        pub struct $name {
            $( pub $ivar: std::cell::RefCell<$crate::RubyValue>, )*
        }

        impl $name {
            pub const CLASS_ID: $crate::ClassId = $crate::ClassId($id);

            pub fn new_handle(inner: Self) -> $crate::RObj {
                std::rc::Rc::new(inner)
            }

            $(
                // `$slf` (not a hardcoded `self`) is captured from the
                // caller's own tokens -- macro_rules! hygiene treats a bare
                // `self` written in this template as distinct from a `self`
                // written inside the caller's `$body`, so the receiver has
                // to round-trip through the caller's tokens to match up.
                //
                // Return type is always `RubyValue`, never omitted: Ruby
                // methods always implicitly return a value (the last
                // expression, or nil), so codegen always emits a body whose
                // final expression is a `RubyValue` -- `RubyValue::Nil` for
                // methods with nothing meaningful to return (e.g.
                // `initialize`), matching Ruby's own semantics rather than
                // introducing a separate "void method" case.
                pub fn $method(& $slf $(, $arg: $arg_ty)*) -> $crate::RubyValue $body
            )*
        }

        impl $crate::RubyObject for $name {
            fn class_id(&self) -> $crate::ClassId { Self::CLASS_ID }
            fn as_any(&self) -> &dyn std::any::Any { self }
        }

        impl $name {
            /// Called once from generated `main()`. Populates the *runtime*
            /// dispatch table (Path 2) with a trampoline per method; Path 1
            /// (static) call sites never go through this table at all.
            pub fn __register(registry: &mut $crate::ClassRegistry) {
                registry.register(Self::CLASS_ID, Some(<$super>::CLASS_ID));
                $(
                    registry.define_method(
                        Self::CLASS_ID,
                        $crate::Symbol::intern(stringify!($method)),
                        |recv, args| {
                            let this = recv.as_any().downcast_ref::<$name>()
                                .expect("class_id guarantees this downcast");
                            #[allow(unused_variables)]
                            match args {
                                [$($arg),*] => $name::$method(this $(, $arg.clone())*),
                                _ => panic!("wrong number of arguments for {}", stringify!($method)),
                            }
                        },
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
            ivars { x }
            def initialize(&self, x: RubyValue) { *self.x.borrow_mut() = x; RubyValue::Nil }
            def x(&self) { self.x.borrow().clone() }
        }
    }

    ruby_class! {
        class Greeter : Object {
            id: 2;
            ivars { }
            def hello(&self) { RubyValue::Str("hi".to_string()) }
            def method_missing(&self, name: RubyValue) {
                RubyValue::Str(format!("no such method: {}", name.to_display_string()))
            }
        }
    }

    fn install() {
        let mut registry = ClassRegistry::new();
        registry.register(Object::CLASS_ID, None);
        Point::__register(&mut registry);
        Greeter::__register(&mut registry);
        install_class_registry(registry);
    }

    #[test]
    fn static_path_stores_and_reads_ivar() {
        let p = Point {
            x: std::cell::RefCell::new(RubyValue::Nil),
        };
        p.initialize(RubyValue::Int(5));
        match p.x() {
            RubyValue::Int(5) => {}
            other => panic!("expected Int(5), got {}", other.to_display_string()),
        }
    }

    #[test]
    fn new_handle_erases_to_a_trait_object() {
        let handle: RObj = Point::new_handle(Point {
            x: std::cell::RefCell::new(RubyValue::Int(7)),
        });
        assert_eq!(handle.class_id(), Point::CLASS_ID);
    }

    #[test]
    fn dynamic_send_finds_registered_method() {
        install();
        let g: RObj = Greeter::new_handle(Greeter {});
        let result = send(&g, Symbol::intern("hello"), &[]);
        assert_eq!(result.to_display_string(), "hi");
    }

    #[test]
    fn dynamic_send_falls_back_to_method_missing() {
        install();
        let g: RObj = Greeter::new_handle(Greeter {});
        let result = send(&g, Symbol::intern("nope"), &[]);
        assert_eq!(result.to_display_string(), "no such method: nope");
    }
}
