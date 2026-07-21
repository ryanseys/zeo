//! `INT_BINARY_OPS`/`INT_UNARY_OPS`/`FLOAT_BINARY_OPS`/`FLOAT_UNARY_OPS` --
//! the native-arithmetic fast-path tables `dispatch` (in the parent module)
//! consults before falling through to ordinary Path 1/Path 2 method
//! dispatch, plus `IntOpKind` (how a table row's runtime fn shapes into an
//! emitted expression) and `emit_int_div_or_mod_checked` (the one row-kind
//! emitter big enough to want its own function, the zero-divisor-checked
//! `int_div`/`int_mod` wrapper).

use quote::{format_ident, quote};

use crate::codegen::Ctx;
use proc_macro2::TokenStream;

/// Binary operators with a native `zeo_rt::int_*` implementation, and
/// which `zeo_rt::RubyValue` variant wraps their result. Every one of
/// these is a plain `CallNode` at the `ruby-prism` level (`a + b` and
/// `a.foo(b)` are the same node shape, just a different `name()`) -- so this
/// table is the entire generalization of the pre-Phase-1 spike's single
/// hardcoded literal-`+`-on-`IntegerLit` fast path: any operand pair
/// statically known `Int` (not just literals -- see
/// `analyze::locals`/`types::infer_type_with_locals`) routes through here;
/// anything else falls through to ordinary Path 1/Path 2 method dispatch
/// below, which is what makes a user class's own `def <=>`/`def +` etc.
/// dispatch correctly instead of needing special-casing here.
pub(super) const INT_BINARY_OPS: &[(&str, &str, IntOpKind)] = &[
    ("+", "int_add", IntOpKind::Value),
    ("-", "int_sub", IntOpKind::Value),
    ("*", "int_mul", IntOpKind::Value),
    ("/", "int_div", IntOpKind::DivMod),
    ("%", "int_mod", IntOpKind::DivMod),
    // `**` is FALLIBLE since the bignum migration: a negative exponent is
    // a Rational result, `0 ** -n` raises ZeroDivisionError.
    ("**", "int_pow", IntOpKind::Fallible),
    ("&", "int_band", IntOpKind::Value),
    ("|", "int_bor", IntOpKind::Value),
    ("^", "int_bxor", IntOpKind::Value),
    // Shifts are fallible too: a beyond-u32 width raises RangeError.
    ("<<", "int_shl", IntOpKind::Fallible),
    (">>", "int_shr", IntOpKind::Fallible),
    ("==", "int_eq", IntOpKind::Bool),
    ("!=", "int_neq", IntOpKind::Bool),
    ("<", "int_lt", IntOpKind::Bool),
    (">", "int_gt", IntOpKind::Bool),
    ("<=", "int_le", IntOpKind::Bool),
    (">=", "int_ge", IntOpKind::Bool),
    ("<=>", "int_cmp", IntOpKind::Cmp),
];
/// How an `INT_BINARY_OPS` row's runtime fn shapes into an emitted
/// expression (Phase 17.1's bignum migration: the `int_*` family takes
/// `&RubyValue` pairs -- an `Int`-typed value may carry either payload --
/// and arithmetic returns `RubyValue` directly, promoting on overflow).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum IntOpKind {
    /// `fn(&RubyValue, &RubyValue) -> RubyValue` -- infallible arithmetic.
    Value,
    /// `fn(..) -> Result<RubyValue, Signal>` -- emitted with `?`.
    Fallible,
    /// Like `Value`, but wrapped in the ZeroDivisionError guard.
    DivMod,
    /// `fn(..) -> bool` -- wrapped in `RubyValue::Bool`.
    Bool,
    /// `fn(..) -> i64` (`<=>`, never nil for Int pairs) -- wrapped in
    /// `RubyValue::Int`.
    Cmp,
}
pub(super) const INT_UNARY_OPS: &[(&str, &str)] =
    &[("-@", "int_neg"), ("+@", "int_pos"), ("~", "int_bnot")];
/// Wraps `zeo_rt::int_div`/`int_mod`'s call with a zero-divisor check,
/// raising a real, catchable `ZeroDivisionError` instead of letting the
/// division/modulo itself hard-panic the whole process -- real Ruby's own
/// behavior (unlike `Float`, where division by zero is `Infinity`/`NaN`/
/// `NaN`, not an error at all -- `float_div`'s own IEEE semantics already
/// give that for free, no check needed there). Found as a real,
/// previously-undetected gap via this session's own testing: `1 / 0`
/// crashed the entire generated binary with a raw Rust panic (`zeo_rt::
/// int_div`'s own internal `/` panicking) rather than raising something a
/// `rescue ZeroDivisionError` could ever catch. A no-op passthrough for
/// every other `rt_fn` (every non-`/`/`%` operator).
pub(super) fn emit_int_div_or_mod_checked(
    cx: &Ctx,
    rt_fn: &str,
    recv_expr: TokenStream,
    arg_expr: TokenStream,
) -> TokenStream {
    let func = format_ident!("{rt_fn}");
    let err = crate::codegen::expr::emit_boxed_new(
        cx,
        "ZeroDivisionError",
        vec![quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new("divided by 0".to_string())) }],
    );
    quote! {
        {
            let __divisor = #arg_expr;
            if zeo_rt::int_is_zero(&__divisor) {
                return Err(zeo_rt::Signal::Raise(#err));
            }
            zeo_rt::#func(&(#recv_expr), &__divisor)
        }
    }
}
/// Same shape as `INT_BINARY_OPS`, minus the bitwise/shift operators (real
/// Ruby's `Float` has none of those) -- `<=>` is deliberately NOT here (see
/// its own dedicated check in `dispatch`): a `Float` comparison against
/// `NaN` returns `nil`, not an `Int`, which this table's uniform
/// "op -> one wrapper variant" shape can't express.
pub(super) const FLOAT_BINARY_OPS: &[(&str, &str, &str)] = &[
    ("+", "float_add", "Float"),
    ("-", "float_sub", "Float"),
    ("*", "float_mul", "Float"),
    ("/", "float_div", "Float"),
    ("%", "float_mod", "Float"),
    ("**", "float_pow", "Float"),
    ("==", "float_eq", "Bool"),
    ("!=", "float_neq", "Bool"),
    ("<", "float_lt", "Bool"),
    (">", "float_gt", "Bool"),
    ("<=", "float_le", "Bool"),
    (">=", "float_ge", "Bool"),
];
pub(super) const FLOAT_UNARY_OPS: &[(&str, &str)] = &[("-@", "float_neg"), ("+@", "float_pos")];
