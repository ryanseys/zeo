//! Between a `VALUE` and a `RubyValue`.
//!
//! Every `rb_*` that takes or returns an object crosses here, so these two
//! functions are the hottest thing in the C surface and the place a mistake
//! is widest. Two rules keep them honest:
//!
//! * [`to_value`] can REFUSE. A Ruby kind whose `RUBY_T_*` tag zeo has not
//!   established has no honest `VALUE`, and answering one anyway would make
//!   `RB_TYPE_P` lie about it forever after. It raises instead, naming the
//!   class.
//! * [`value_of`] cannot refuse, and does not try. A `VALUE` that is not an
//!   immediate and not a live handle is already undefined behaviour by the
//!   time it arrives; there is no value to answer and no caller to answer it
//!   to.

use super::handles;
use super::value::{self, Value};
use crate::{RubyValue, Signal};

/// The `VALUE` for `v`, pinned in the current scope.
///
/// The pin is what keeps the object alive while C holds the `VALUE`; see
/// [`super::scope`] for why that is a scope rather than a stack scan.
pub fn to_value(v: &RubyValue) -> Result<Value, Signal> {
    handles::pin(v).ok_or_else(|| {
        crate::builtins::not_impl_error!(
            "zeo cannot hand a {} to a C extension yet: it has no established \
                 RUBY_T_* tag, and guessing one would make RB_TYPE_P wrong",
            crate::dispatch::class_name(v.class_id()).unwrap_or("value".into())
        )
    })
}

/// The Ruby value a `VALUE` names.
///
/// # Safety
///
/// `v` must be an immediate or a handle that is still pinned. Every caller is
/// a `rb_*` entry point, and the extension's own `VALUE` is the argument.
pub unsafe fn value_of(v: Value) -> RubyValue {
    if let Some(imm) = value::from_immediate(v) {
        return imm;
    }
    if v == value::Q_UNDEF {
        // `Qundef` is MRI's "no value here" marker and never a Ruby object.
        // An extension that hands one to a function expecting an object is
        // doing something MRI would also refuse; nil is the closest honest
        // answer and the caller's own type check is what catches it.
        return RubyValue::Nil;
    }
    // SAFETY: the caller's contract. `from_immediate` has ruled out every
    // encoding that is not a pointer.
    unsafe { handles::deref(v).value().clone() }
}

/// `RTEST`: everything but `false` and `nil` is true.
pub const fn truthy(v: Value) -> bool {
    v != value::Q_FALSE && v != value::Q_NIL
}

pub const fn boolean(b: bool) -> Value {
    if b { value::Q_TRUE } else { value::Q_FALSE }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_immediate_round_trips_without_a_handle() {
        let _scope = super::super::scope::Scope::enter();
        let before = handles::live_count();
        for v in [
            RubyValue::Nil,
            RubyValue::Bool(true),
            RubyValue::Bool(false),
            RubyValue::Int(7),
            RubyValue::Int(-7),
        ] {
            let raw = to_value(&v).expect("an immediate always converts");
            assert!(value::is_special_const(raw), "{v:?} took a handle");
            assert_eq!(format!("{:?}", unsafe { value_of(raw) }), format!("{v:?}"));
        }
        assert_eq!(
            handles::live_count(),
            before,
            "an immediate minted a handle"
        );
    }

    #[test]
    fn a_heap_value_round_trips_through_its_handle() {
        let _scope = super::super::scope::Scope::enter();
        let s = crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, "round");
        let raw = to_value(&s).expect("a String converts");
        let back = unsafe { value_of(raw) };
        match (&s, &back) {
            (RubyValue::Str(a), RubyValue::Str(b)) => {
                assert!(std::sync::Arc::ptr_eq(a, b), "the round trip copied");
            }
            _ => panic!("a String did not come back a String"),
        }
    }

    #[test]
    fn rtest_matches_rubys_own_truthiness() {
        assert!(!truthy(value::Q_FALSE));
        assert!(!truthy(value::Q_NIL));
        assert!(truthy(value::Q_TRUE));
        assert!(truthy(value::fixnum(0)), "0 is true in ruby");
        assert_eq!(boolean(true), value::Q_TRUE);
        assert_eq!(boolean(false), value::Q_FALSE);
    }
}
