//! The fixed-arity dispatch trampolines, as one exported macro instead of a
//! ~10-line closure per method in the generated program.
//!
//! `codegen`'s `emit_dynamic_trampoline`/`emit_value_trampoline` emit a
//! `zeo_tramp!` invocation whenever a method's signature is PLAIN -- required
//! positionals only (no optionals, rest, post, or keywords; a `&block`
//! parameter is fine) -- which is most methods in real code. Every other
//! signature keeps the long-form closure those emitters build. The expansion
//! here is token-for-token what the long form produces for the same
//! signature: exact-count arity check (raising inside the callee's frame when
//! the def has a location), then positional clones.
//!
//! Heads: `inst` downcasts to a generated struct and calls its inherent
//! method; `pass` forwards the receiver `RubyValue` to a free function (a
//! builtin-reopen body); `drop` calls a free function with no receiver (a
//! class method -- which class it is, is baked into the path). The `b`-carrying
//! variants forward the call-site block to a `&block`-taking method.
//!
//! `rd`/`wr` are the ACCESSOR heads, for a method whose whole body is one ivar
//! access (`compiler::AccessorShape`). They reach the field directly instead of
//! calling the generated inherent method, which drops four costs the `inst`
//! head pays for a body that cannot use any of them: `downcast_robj`'s `Arc`
//! clone and matching drop (4.08 ns measured -- most of an accessor call), the
//! `Arc<Self>` handed to the callee, the callee's frame push/pop, and its
//! `check_ints`. This is what devirtualizes a `Poly` receiver -- one whose
//! concrete class codegen could not name, which is what `bm_splay`,
//! `bm_rbtree` and `bm_linked_list` actually hold -- since those never reach
//! the static call site at all and can only be reached here.

/// See the module docs. `$frame` is the callee's `let __frame = ...` guard
/// STATEMENT (no trailing semicolon), present only when the def has a source
/// location; it must live in the raising scope so the error's backtrace
/// carries the callee's frame.
#[macro_export]
macro_rules! zeo_tramp {
    (inst $ty:ty, $meth:ident, $n:literal, [$($ix:literal),*] $(, $frame:stmt)?) => {
        |recv: &$crate::RObj, args: &[$crate::RubyValue], _blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            let this = $crate::downcast_robj::<$ty>(recv)
                .expect("class_id guarantees this downcast");
            if args.len() != $n {
                $($frame;)?
                return Err($crate::arity_error(args.len(), $n));
            }
            <$ty>::$meth(this, $(args[$ix].clone(),)*)
        }
    };
    (instb $ty:ty, $meth:ident, $n:literal, [$($ix:literal),*] $(, $frame:stmt)?) => {
        |recv: &$crate::RObj, args: &[$crate::RubyValue], blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            let this = $crate::downcast_robj::<$ty>(recv)
                .expect("class_id guarantees this downcast");
            if args.len() != $n {
                $($frame;)?
                return Err($crate::arity_error(args.len(), $n));
            }
            <$ty>::$meth(this, $(args[$ix].clone(),)* blk)
        }
    };
    (rd $ty:ty, $field:ident $(, $frame:stmt)?) => {
        |recv: &$crate::RObj, args: &[$crate::RubyValue], _blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            if !args.is_empty() {
                $($frame;)?
                return Err($crate::arity_error(args.len(), 0));
            }
            let this = $crate::downcast_robj_ref::<$ty>(recv)
                .expect("class_id guarantees this downcast");
            // `unwrap_or(Nil)` for the same reason the static read does it:
            // the slot is `Option`-shaped so `defined?` can tell "never
            // assigned" from "assigned nil", but a READ is `nil` either way.
            Ok(this.$field.lock().clone().unwrap_or($crate::RubyValue::Nil))
        }
    };
    (wr $ty:ty, $field:ident $(, $frame:stmt)?) => {
        |recv: &$crate::RObj, args: &[$crate::RubyValue], _blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            if args.len() != 1 {
                $($frame;)?
                return Err($crate::arity_error(args.len(), 1));
            }
            let this = $crate::downcast_robj_ref::<$ty>(recv)
                .expect("class_id guarantees this downcast");
            if $crate::RubyObject::is_frozen(&**recv) {
                $($frame;)?
                return Err($crate::ivar_frozen_error(recv.clone()));
            }
            *this.$field.lock() = Some(args[0].clone());
            // A writer's value is its ARGUMENT, not the assignment's target --
            // `(o.x = 1)` is `1` even where `x=` returns something else.
            Ok(args[0].clone())
        }
    };
    (pass $path:path, $n:literal, [$($ix:literal),*] $(, $frame:stmt)?) => {
        |recv: &$crate::RubyValue, args: &[$crate::RubyValue], _blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            if args.len() != $n {
                $($frame;)?
                return Err($crate::arity_error(args.len(), $n));
            }
            $path(recv.clone(), $(args[$ix].clone(),)*)
        }
    };
    (passb $path:path, $n:literal, [$($ix:literal),*] $(, $frame:stmt)?) => {
        |recv: &$crate::RubyValue, args: &[$crate::RubyValue], blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            if args.len() != $n {
                $($frame;)?
                return Err($crate::arity_error(args.len(), $n));
            }
            $path(recv.clone(), $(args[$ix].clone(),)* blk)
        }
    };
    (drop $path:path, $n:literal, [$($ix:literal),*] $(, $frame:stmt)?) => {
        |_recv: &$crate::RubyValue, args: &[$crate::RubyValue], _blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            if args.len() != $n {
                $($frame;)?
                return Err($crate::arity_error(args.len(), $n));
            }
            $path($(args[$ix].clone(),)*)
        }
    };
    (dropb $path:path, $n:literal, [$($ix:literal),*] $(, $frame:stmt)?) => {
        |_recv: &$crate::RubyValue, args: &[$crate::RubyValue], blk: Option<$crate::RubyValue>|
            -> Result<$crate::RubyValue, $crate::Signal> {
            if args.len() != $n {
                $($frame;)?
                return Err($crate::arity_error(args.len(), $n));
            }
            $path($(args[$ix].clone(),)* blk)
        }
    };
}
