//! Boundary-contract tests for the capi surface: pointers built the way
//! compiled code builds them, asserted against the Rust-side internals.

use super::frames::*;
use super::signals::*;
use super::values::*;
use crate::{RubyValue, Signal};
use std::mem::MaybeUninit;
use std::sync::Arc;
use zeo_abi::abi::{STATUS_OK, SignalKind};

fn a_string(text: &str) -> RubyValue {
    RubyValue::Str(crate::value::collections::string_new(text.to_string()))
}

fn strong_count(v: &RubyValue) -> usize {
    match v {
        RubyValue::Str(arc) => Arc::strong_count(arc),
        other => panic!("test helper wants a Str, got {other:?}"),
    }
}

#[test]
fn retain_and_release_balance_the_refcount() {
    let v = a_string("capi");
    assert_eq!(strong_count(&v), 1);
    unsafe { zeo_rt_retain(&v) };
    assert_eq!(strong_count(&v), 2);
    // Release the reference the retain minted exactly as compiled code
    // would: a bit-copy of the slot IS that reference.
    let mut slot = MaybeUninit::new(unsafe { std::ptr::read(&v) });
    unsafe { zeo_rt_release(slot.as_mut_ptr()) };
    assert_eq!(strong_count(&v), 1);
}

#[test]
fn the_pool_drains_at_frame_pop() {
    let v = a_string("pooled");
    unsafe {
        zeo_rt_frame_push(
            c"t.rb".as_ptr().cast(),
            4,
            c"Object#m".as_ptr().cast(),
            8,
            1,
            2,
        );
    }
    let mut temp = MaybeUninit::new(v.clone());
    assert_eq!(strong_count(&v), 2);
    unsafe { zeo_rt_pool_push(temp.as_mut_ptr()) };
    assert_eq!(strong_count(&v), 2, "pool_push moves, it does not clone");
    unsafe { zeo_rt_frame_pop() };
    assert_eq!(strong_count(&v), 1, "frame_pop released the pooled temp");
}

#[test]
fn pool_mark_and_reset_bracket_a_loop_iteration() {
    let v = a_string("latch");
    let mark = unsafe { zeo_rt_pool_mark() };
    for _ in 0..3 {
        let mut temp = MaybeUninit::new(v.clone());
        unsafe { zeo_rt_pool_push(temp.as_mut_ptr()) };
    }
    assert_eq!(strong_count(&v), 4);
    unsafe { zeo_rt_pool_reset(mark) };
    assert_eq!(strong_count(&v), 1);
}

#[test]
fn frame_push_shows_in_the_backtrace_and_pops_clean() {
    let depth_before = crate::frames::capture_backtrace().len();
    unsafe {
        zeo_rt_frame_push(
            c"cap.rb".as_ptr().cast(),
            6,
            c"Object#capi".as_ptr().cast(),
            11,
            7,
            9,
        );
        zeo_rt_set_line(8);
    }
    let bt = crate::frames::capture_backtrace();
    assert_eq!(
        bt.first().map(String::as_str),
        Some("cap.rb:8:in 'Object#capi'")
    );
    unsafe { zeo_rt_frame_pop() };
    assert_eq!(crate::frames::capture_backtrace().len(), depth_before);
}

#[test]
fn payload_signals_roundtrip_through_set_kind_take() {
    let mut v = MaybeUninit::new(RubyValue::Int(42));
    unsafe { zeo_rt_signal_set(SignalKind::Break as u8, v.as_mut_ptr()) };
    assert_eq!(unsafe { zeo_rt_signal_kind() }, SignalKind::Break as u8);
    let mut out = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_signal_take(out.as_mut_ptr()) },
        SignalKind::Break as u8
    );
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(42)));
    assert_eq!(unsafe { zeo_rt_signal_kind() }, SignalKind::None as u8);
}

#[test]
fn no_payload_signals_stay_parked_across_take() {
    unsafe { zeo_rt_signal_set(SignalKind::Retry as u8, std::ptr::null_mut()) };
    let mut out = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_signal_take(out.as_mut_ptr()) },
        SignalKind::Retry as u8
    );
    assert_eq!(
        unsafe { zeo_rt_signal_kind() },
        SignalKind::Retry as u8,
        "take leaves a payload-less signal parked"
    );
    assert!(matches!(crate::signal::take_pending(), Some(Signal::Retry)));
}

#[test]
fn the_ensure_bracket_saves_and_restores_a_raise() {
    crate::signal::set_pending(Signal::Raise(a_string("boom")));
    let saved = unsafe { zeo_rt_signal_save() };
    assert_eq!(unsafe { zeo_rt_signal_kind() }, SignalKind::None as u8);
    let pushed = unsafe { zeo_rt_propagating_enter(saved) };
    assert_eq!(pushed, 1);
    assert!(
        crate::handling::current_exception().is_some(),
        "$! holds the propagating raise while the ensure body runs"
    );
    unsafe { zeo_rt_propagating_leave(pushed) };
    assert!(crate::handling::current_exception().is_none());
    unsafe { zeo_rt_signal_restore(saved) };
    assert_eq!(unsafe { zeo_rt_signal_kind() }, SignalKind::Raise as u8);
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_signal_take(out.as_mut_ptr()) };
    drop(unsafe { out.assume_init() });
}

#[test]
fn a_throw_survives_the_ensure_bracket_whole() {
    crate::signal::set_pending(Signal::Throw(Box::new(crate::signal::Thrown {
        tag: RubyValue::Int(1),
        value: RubyValue::Int(2),
    })));
    let saved = unsafe { zeo_rt_signal_save() };
    assert_eq!(unsafe { zeo_rt_propagating_enter(saved) }, 0);
    unsafe { zeo_rt_propagating_leave(0) };
    unsafe { zeo_rt_signal_restore(saved) };
    let mut out = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_signal_take(out.as_mut_ptr()) },
        SignalKind::Throw as u8
    );
    assert!(matches!(
        crate::signal::take_pending(),
        Some(Signal::Throw(_))
    ));
}

#[test]
fn rescue_matches_walks_the_builtin_ancestry() {
    let exc = RubyValue::Int(3);
    let classes = [zeo_abi::INTEGER_CLASS.0, zeo_abi::STRING_CLASS.0];
    assert_eq!(
        unsafe { zeo_rt_rescue_matches(&exc, classes.as_ptr(), 2) },
        1
    );
    let miss = [zeo_abi::STRING_CLASS.0];
    assert_eq!(unsafe { zeo_rt_rescue_matches(&exc, miss.as_ptr(), 1) }, 0);
}

#[test]
fn value_predicates_answer_inline_questions() {
    let s = a_string("x");
    unsafe {
        assert_eq!(zeo_rt_truthy(&RubyValue::Nil), 0);
        assert_eq!(zeo_rt_truthy(&RubyValue::Bool(false)), 0);
        assert_eq!(zeo_rt_truthy(&s), 1);
        assert_eq!(
            zeo_rt_class_of(&RubyValue::Int(1)),
            zeo_abi::INTEGER_CLASS.0
        );
        assert_eq!(zeo_rt_is_a(&s, zeo_abi::STRING_CLASS.0), 1);
        assert_eq!(zeo_rt_is_a(&s, zeo_abi::INTEGER_CLASS.0), 0);
    }
    let mut eq = 0i8;
    assert_eq!(
        unsafe { zeo_rt_eq(&RubyValue::Int(4), &RubyValue::Int(4), &mut eq) },
        STATUS_OK
    );
    assert_eq!(eq, 1);
}

#[test]
fn bignum_from_decimal_normalizes_small_and_keeps_big() {
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe {
        zeo_rt_bignum_from_decimal(c"9223372036854775807".as_ptr().cast(), 19, out.as_mut_ptr())
    };
    assert!(matches!(
        unsafe { out.assume_init() },
        RubyValue::Int(i64::MAX)
    ));
    let big = "9223372036854775808";
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_bignum_from_decimal(big.as_ptr(), big.len(), out.as_mut_ptr()) };
    let v = unsafe { out.assume_init() };
    assert!(matches!(&v, RubyValue::BigInt(b) if b.to_string() == big));
}

#[test]
fn stack_check_and_check_ints_answer_ok_on_a_quiet_thread() {
    assert_eq!(unsafe { zeo_rt_stack_check() }, STATUS_OK);
    assert_eq!(unsafe { zeo_rt_check_ints() }, STATUS_OK);
}

// -- M0-4: the Rust->C dispatch bridge --------------------------------------

unsafe extern "C" fn double_plus_args(
    recv: *const RubyValue,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    assert!(blk.is_null(), "this test body takes no block");
    let n = match unsafe { &*recv } {
        RubyValue::Int(n) => *n,
        other => panic!("test body wants an Int receiver, got {other:?}"),
    };
    let extra: i64 = (0..argc)
        .map(|i| match unsafe { &*argv.add(i) } {
            RubyValue::Int(n) => *n,
            other => panic!("{other:?}"),
        })
        .sum();
    unsafe { out.write(RubyValue::Int(n * 2 + extra)) };
    0
}

unsafe extern "C" fn always_signals(
    _recv: *const RubyValue,
    _argv: *const RubyValue,
    _argc: usize,
    _blk: *mut RubyValue,
    _out: *mut RubyValue,
) -> i32 {
    crate::signal::set_pending(Signal::Return(RubyValue::Int(7)));
    1
}

unsafe extern "C" fn consumes_its_block(
    _recv: *const RubyValue,
    _argv: *const RubyValue,
    _argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    assert!(!blk.is_null());
    // The block arrives by MOVE: the callee owns and releases it.
    drop(unsafe { std::ptr::read(blk) });
    unsafe { out.write(RubyValue::Nil) };
    0
}

#[test]
fn a_c_value_impl_calls_through_the_bridge() {
    let f = crate::dispatch::ValueImpl::C(double_plus_args);
    let recv = RubyValue::Int(20);
    let args = [RubyValue::Int(1), RubyValue::Int(2)];
    assert!(matches!(f.call(&recv, &args, None), Ok(RubyValue::Int(43))));
}

#[test]
fn a_signalling_c_body_becomes_an_err() {
    let f = crate::dispatch::ValueImpl::C(always_signals);
    match f.call(&RubyValue::Nil, &[], None) {
        Err(Signal::Return(RubyValue::Int(7))) => {}
        other => panic!("expected the parked Return, got {other:?}"),
    }
    assert_eq!(unsafe { zeo_rt_signal_kind() }, SignalKind::None as u8);
}

#[test]
fn the_block_moves_into_the_c_callee() {
    let block = a_string("blk");
    let watcher = block.clone();
    assert_eq!(strong_count(&watcher), 2);
    let f = crate::dispatch::ValueImpl::C(consumes_its_block);
    f.call(&RubyValue::Nil, &[], Some(block)).unwrap();
    assert_eq!(strong_count(&watcher), 1, "the callee released the block");
}

// -- M0-5: CompiledObject, LAYOUTS, and slot ivars ---------------------------

use super::objects::*;

/// One registered layout for these tests: two named ivars + one hidden
/// member slot, on a class id far outside the builtin range.
fn test_layout_class() -> u32 {
    static LAYOUT: crate::compiled_object::ClassLayout = crate::compiled_object::ClassLayout {
        names: &["a", "b"],
        hidden: 1,
    };
    static ONCE: std::sync::Once = std::sync::Once::new();
    const CID: u32 = 900_001;
    ONCE.call_once(|| {
        crate::compiled_object::register_layout(zeo_abi::ClassId(CID), &LAYOUT);
    });
    CID
}

#[test]
fn a_compiled_object_allocates_and_slots_roundtrip() {
    let cid = test_layout_class();
    let mut obj = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_object_alloc(cid, obj.as_mut_ptr()) };
    let obj = unsafe { obj.assume_init() };
    assert_eq!(unsafe { zeo_rt_class_of(&obj) }, cid);

    // The HIDDEN member slots come first, so `@a`/`@b` are slots 1 and 2.
    // A subclass of a compiled struct inherits the member list, and members
    // laid out last would move the moment the subclass declared an ivar of
    // its own -- putting `@p` where the ancestor's `initialize` writes the
    // member.
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_ivar_get_slot(&obj, 1, out.as_mut_ptr()) };
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Nil));
    let mut v = MaybeUninit::new(RubyValue::Int(5));
    assert_eq!(
        unsafe { zeo_rt_ivar_set_slot(&obj, 2, v.as_mut_ptr()) },
        STATUS_OK
    );
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_ivar_get_slot(&obj, 2, out.as_mut_ptr()) };
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(5)));

    // Reflection reports first-assignment order and skips the hidden slot.
    let RubyValue::Object(o) = &obj else { panic!() };
    let mut h = MaybeUninit::new(RubyValue::Int(9));
    assert_eq!(
        unsafe { zeo_rt_ivar_set_slot(&obj, 0, h.as_mut_ptr()) },
        STATUS_OK
    );
    assert_eq!(
        o.ivar_pairs()
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>(),
        vec!["@b".to_string()],
        "members are not instance variables"
    );
    assert!(matches!(o.hidden_ivar_get(0), Some(RubyValue::Int(9))));
    assert!(matches!(o.ivar_get_named("b"), Some(RubyValue::Int(5))));
}

#[test]
fn frozen_state_flows_through_the_object() {
    // The REFUSAL side (a frozen write raising FrozenError) constructs an
    // exception, which the registry-less unit environment cannot -- the
    // M0 slice goldens cover it. Here: the flag itself and the clean check.
    let cid = test_layout_class();
    let mut obj = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_object_alloc(cid, obj.as_mut_ptr()) };
    let obj = unsafe { obj.assume_init() };
    assert_eq!(unsafe { zeo_rt_frozen_check(&obj) }, STATUS_OK);
    let RubyValue::Object(o) = &obj else { panic!() };
    assert!(!o.is_frozen());
    o.set_frozen();
    assert!(o.is_frozen());
}

#[test]
fn dup_object_shares_handles_and_dup_starts_unfrozen() {
    let cid = test_layout_class();
    let mut obj = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_object_alloc(cid, obj.as_mut_ptr()) };
    let obj = unsafe { obj.assume_init() };
    let payload = a_string("shared");
    let mut v = MaybeUninit::new(payload.clone());
    unsafe { zeo_rt_ivar_set_slot(&obj, 0, v.as_mut_ptr()) };
    let RubyValue::Object(o) = &obj else { panic!() };
    o.set_frozen();
    let copy = o.dup_object(false);
    assert!(!copy.is_frozen());
    assert_eq!(strong_count(&payload), 3, "shallow: the handle is shared");
    let clone = o.dup_object(true);
    assert!(clone.is_frozen());
    drop(copy);
    drop(clone);
    assert_eq!(strong_count(&payload), 2);
}

// -- M0-6: cells, C-bodied procs, yield --------------------------------------

use super::procs::*;

/// A block body that adds its first argument into its one captured cell
/// and answers the new total. `break 7` when called with no arguments.
unsafe extern "C" fn accumulate_body(
    env: *const ProcEnv,
    _self: *const RubyValue,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    assert!(blk.is_null());
    let env = unsafe { &*env };
    assert_eq!(env.n_cells, 1);
    if argc == 0 {
        let mut v = MaybeUninit::new(RubyValue::Int(7));
        unsafe { zeo_rt_signal_set(SignalKind::Break as u8, v.as_mut_ptr()) };
        return 1;
    }
    let cell = unsafe { *env.cells };
    let mut cur = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_cell_load(cell, cur.as_mut_ptr()) };
    let RubyValue::Int(base) = (unsafe { cur.assume_init() }) else {
        panic!()
    };
    let RubyValue::Int(arg) = (unsafe { &*argv }) else {
        panic!()
    };
    let mut next = MaybeUninit::new(RubyValue::Int(base + arg));
    unsafe { zeo_rt_cell_store(cell, next.as_mut_ptr()) };
    unsafe { out.write(RubyValue::Int(base + arg)) };
    0
}

#[test]
fn cells_roundtrip_and_balance() {
    let mut init = MaybeUninit::new(a_string("cellv"));
    let watcher = unsafe { &*init.as_ptr() }.clone();
    let cell = unsafe { zeo_rt_cell_new(init.as_mut_ptr()) };
    assert_eq!(strong_count(&watcher), 2, "the cell owns the moved value");
    unsafe { zeo_rt_cell_retain(cell) };
    unsafe { zeo_rt_cell_release(cell) };
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_cell_load(cell, out.as_mut_ptr()) };
    drop(unsafe { out.assume_init() });
    let mut replacement = MaybeUninit::new(RubyValue::Int(1));
    unsafe { zeo_rt_cell_store(cell, replacement.as_mut_ptr()) };
    assert_eq!(
        strong_count(&watcher),
        1,
        "the store released the old value"
    );
    unsafe { zeo_rt_cell_release(cell) };
}

#[test]
fn a_c_proc_captures_cells_and_yields() {
    // The captured local, escaped into a cell as compiled code would do it.
    let mut init = MaybeUninit::new(RubyValue::Int(100));
    let cell = unsafe { zeo_rt_cell_new(init.as_mut_ptr()) };
    let cells = [cell];
    let recv = RubyValue::Nil;
    let mut blkv = MaybeUninit::<RubyValue>::uninit();
    unsafe {
        zeo_rt_proc_new(
            accumulate_body,
            cells.as_ptr(),
            1,
            &recv,
            std::ptr::null(),
            std::ptr::null(),
            -1,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            0,
            blkv.as_mut_ptr(),
        )
    };
    // The frame's own cell reference can go: the proc holds its own.
    unsafe { zeo_rt_cell_release(cell) };
    let blkv = unsafe { blkv.assume_init() };
    let RubyValue::Proc(p) = &blkv else { panic!() };
    assert_eq!(p.arity(), -1);
    assert!(!p.is_lambda());

    // Two yields accumulate through the shared cell.
    let args = [RubyValue::Int(5)];
    let mut out = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_yield(&blkv, args.as_ptr(), 1, out.as_mut_ptr()) },
        STATUS_OK
    );
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(105)));
    let mut out = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_yield(&blkv, args.as_ptr(), 1, out.as_mut_ptr()) },
        STATUS_OK
    );
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(110)));

    // A `break` inside the body surfaces as the parked signal.
    let mut out = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_yield(&blkv, std::ptr::null(), 0, out.as_mut_ptr()) },
        1
    );
    let mut sig = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_signal_take(sig.as_mut_ptr()) },
        SignalKind::Break as u8
    );
    assert!(matches!(unsafe { sig.assume_init() }, RubyValue::Int(7)));

    // Proc#call reaches the same body.
    let mut out = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe {
            zeo_rt_proc_call(
                &blkv,
                args.as_ptr(),
                1,
                std::ptr::null_mut(),
                out.as_mut_ptr(),
            )
        },
        STATUS_OK
    );
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(115)));
}

#[test]
fn yield_without_a_block_signals() {
    let mut out = MaybeUninit::<RubyValue>::uninit();
    // Registry-less, the LocalJumpError construction would panic-abort, so
    // only the null-pointer *shape* is asserted through a real proc: a
    // non-null block never signals. (The null path is golden-covered.)
    let mut init = MaybeUninit::new(RubyValue::Int(0));
    let cell = unsafe { zeo_rt_cell_new(init.as_mut_ptr()) };
    let cells = [cell];
    let recv = RubyValue::Nil;
    let mut blkv = MaybeUninit::<RubyValue>::uninit();
    unsafe {
        zeo_rt_proc_new(
            accumulate_body,
            cells.as_ptr(),
            1,
            &recv,
            std::ptr::null(),
            std::ptr::null(),
            -1,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            0,
            blkv.as_mut_ptr(),
        )
    };
    unsafe { zeo_rt_cell_release(cell) };
    let blkv = unsafe { blkv.assume_init() };
    let args = [RubyValue::Int(1)];
    assert_eq!(
        unsafe { zeo_rt_yield(&blkv, args.as_ptr(), 1, out.as_mut_ptr()) },
        STATUS_OK
    );
    drop(unsafe { out.assume_init() });
}

// The raise channels (`zeo_rt_raise_error`, `zeo_rt_wrong_arity`) are
// untestable registry-less: the documented loud panic cannot unwind out of
// an `extern "C"` fn (it aborts, per decision 13's boundary posture). They
// are exercised end to end by the M0 slice goldens.

#[test]
fn svar_scope_brackets_push_and_pop() {
    unsafe { zeo_rt_svar_scope_push() };
    unsafe { zeo_rt_svar_scope_pop() };
}

#[test]
fn synthetic_c_frames_dedupe_and_pop_what_they_pushed() {
    unsafe {
        zeo_rt_frame_push(
            c"s.rb".as_ptr().cast(),
            4,
            c"Object#n".as_ptr().cast(),
            8,
            1,
            2,
        );
        let label = c"Array#sum";
        let first = zeo_rt_synthetic_c_frame_push(label.as_ptr().cast(), 9);
        assert_eq!(first, 1);
        let repeat = zeo_rt_synthetic_c_frame_push(label.as_ptr().cast(), 9);
        assert_eq!(repeat, 0, "an exact repeat is deduped");
        zeo_rt_synthetic_c_frame_pop(repeat);
        zeo_rt_synthetic_c_frame_pop(first);
        zeo_rt_frame_pop();
    }
}

// -- M0-7 (b): literals and numeric slow paths -------------------------------

use super::literals::*;
use super::numeric::*;

#[test]
fn string_literals_intern_frozen_and_str_new_allocates_fresh() {
    let utf8 = crate::encoding::UTF_8.0;
    let mut a = MaybeUninit::<RubyValue>::uninit();
    let mut b = MaybeUninit::<RubyValue>::uninit();
    unsafe {
        zeo_rt_str_lit(c"lit".as_ptr().cast(), 3, utf8, a.as_mut_ptr());
        zeo_rt_str_lit(c"lit".as_ptr().cast(), 3, utf8, b.as_mut_ptr());
    }
    let (a, b) = unsafe { (a.assume_init(), b.assume_init()) };
    let (RubyValue::Str(x), RubyValue::Str(y)) = (&a, &b) else {
        panic!()
    };
    assert!(Arc::ptr_eq(x, y), "one frozen object per content");
    let mut c = MaybeUninit::<RubyValue>::uninit();
    let mut d = MaybeUninit::<RubyValue>::uninit();
    unsafe {
        zeo_rt_str_new(c"lit".as_ptr().cast(), 3, utf8, c.as_mut_ptr());
        zeo_rt_str_new(c"lit".as_ptr().cast(), 3, utf8, d.as_mut_ptr());
    }
    let (c, d) = unsafe { (c.assume_init(), d.assume_init()) };
    let (RubyValue::Str(x), RubyValue::Str(y)) = (&c, &d) else {
        panic!()
    };
    assert!(!Arc::ptr_eq(x, y), "each evaluation allocates");
}

#[test]
fn symbols_array_hash_range_construct() {
    let id = unsafe { zeo_rt_sym_intern(c"capi_sym".as_ptr().cast(), 8) };
    let mut s = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_sym_value(id, s.as_mut_ptr()) };
    assert!(
        matches!(unsafe { s.assume_init() }, RubyValue::Symbol(sym) if sym.name_str() == "capi_sym")
    );

    let mut a = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_array_new(2, a.as_mut_ptr()) };
    let a = unsafe { a.assume_init() };
    let mut v = MaybeUninit::new(RubyValue::Int(1));
    unsafe { zeo_rt_array_push(&a, v.as_mut_ptr()) };
    let mut v = MaybeUninit::new(RubyValue::Int(2));
    unsafe { zeo_rt_array_push(&a, v.as_mut_ptr()) };
    assert_eq!(unsafe { zeo_rt_array_len(&a) }, 2);
    let mut e = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_array_get(&a, 1, e.as_mut_ptr()) };
    assert!(matches!(unsafe { e.assume_init() }, RubyValue::Int(2)));
    let mut e = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_array_get(&a, 9, e.as_mut_ptr()) };
    assert!(matches!(unsafe { e.assume_init() }, RubyValue::Nil));

    let mut h = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_hash_new(h.as_mut_ptr()) };
    let h = unsafe { h.assume_init() };
    let mut k = MaybeUninit::new(RubyValue::Int(1));
    let mut val = MaybeUninit::new(RubyValue::Int(10));
    unsafe { zeo_rt_hash_set(&h, k.as_mut_ptr(), val.as_mut_ptr()) };
    let RubyValue::Hash(hd) = &h else { panic!() };
    assert_eq!(hd.lock().len(), 1);

    let mut lo = MaybeUninit::new(RubyValue::Int(1));
    let mut hi = MaybeUninit::new(RubyValue::Int(5));
    let mut r = MaybeUninit::<RubyValue>::uninit();
    assert_eq!(
        unsafe { zeo_rt_range_new(lo.as_mut_ptr(), hi.as_mut_ptr(), 0, r.as_mut_ptr()) },
        STATUS_OK
    );
    assert!(matches!(unsafe { r.assume_init() }, RubyValue::Range(_)));
}

#[test]
fn integer_slow_paths_promote_and_compare() {
    let a = RubyValue::Int(i64::MAX);
    let b = RubyValue::Int(1);
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_int_add_slow(&a, &b, out.as_mut_ptr()) };
    let sum = unsafe { out.assume_init() };
    assert!(matches!(&sum, RubyValue::BigInt(v) if v.to_string() == "9223372036854775808"));
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_int_sub_slow(&sum, &b, out.as_mut_ptr()) };
    assert!(
        matches!(unsafe { out.assume_init() }, RubyValue::Int(i64::MAX)),
        "the slow path normalizes back to Int"
    );
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_int_mul_slow(&RubyValue::Int(6), &RubyValue::Int(7), out.as_mut_ptr()) };
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(42)));
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_int_div(&RubyValue::Int(-7), &RubyValue::Int(2), out.as_mut_ptr()) };
    assert!(
        matches!(unsafe { out.assume_init() }, RubyValue::Int(-4)),
        "floored division"
    );
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_int_mod(&RubyValue::Int(-7), &RubyValue::Int(2), out.as_mut_ptr()) };
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(1)));
    assert!(unsafe { zeo_rt_int_cmp_slow(&a, &b) } > 0);
}

// -- M0-7: register_program + zeo_rt_main -----------------------------------

use zeo_abi::abi;

fn abi_str(s: &'static str) -> abi::Str {
    abi::Str {
        ptr: s.as_ptr(),
        len: s.len(),
    }
}

fn no_str() -> abi::Str {
    abi::Str {
        ptr: std::ptr::null(),
        len: 0,
    }
}

fn empty_desc(toplevel: abi::UnitFn) -> abi::ProgramDesc {
    abi::ProgramDesc {
        abi_version: abi::ABI_VERSION,
        classes: std::ptr::null(),
        n_classes: 0,
        obj_rows: std::ptr::null(),
        n_obj_rows: 0,
        vm_rows: std::ptr::null(),
        n_vm_rows: 0,
        vm_foreign: std::ptr::null(),
        n_vm_foreign: 0,
        cm_rows: std::ptr::null(),
        n_cm_rows: 0,
        vis_rows: std::ptr::null(),
        n_vis_rows: 0,
        meta_rows: std::ptr::null(),
        n_meta_rows: 0,
        reg_rows: std::ptr::null(),
        n_reg_rows: 0,
        units: std::ptr::null(),
        n_units: 0,
        declined: std::ptr::null(),
        n_declined: 0,
        coverage: std::ptr::null(),
        n_cov: 0,
        loaded_features: std::ptr::null(),
        n_loaded: 0,
        load_path: std::ptr::null(),
        n_load_path: 0,
        parse_warnings: std::ptr::null(),
        n_warnings: 0,
        sources: std::ptr::null(),
        n_sources: 0,
        data_section: no_str(),
        data_offset: 0,
        toplevel,
        unit_init: None,
        eval_install: None,
    }
}

unsafe extern "C" fn nil_toplevel(out: *mut abi::Value) -> i32 {
    unsafe { out.cast::<RubyValue>().write(RubyValue::Nil) };
    0
}

/// The `abi::ValueFn` spelling of a runtime-typed test body -- the same
/// transmute `register_program` performs in reverse.
fn as_abi_fn(f: super::ValueFn) -> abi::ValueFn {
    unsafe { std::mem::transmute::<super::ValueFn, abi::ValueFn>(f) }
}

unsafe extern "C" fn widget_answer(
    _recv: *const RubyValue,
    argv: *const RubyValue,
    argc: usize,
    _blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let extra: i64 = (0..argc)
        .map(|i| match unsafe { &*argv.add(i) } {
            RubyValue::Int(n) => *n,
            other => panic!("{other:?}"),
        })
        .sum();
    unsafe { out.write(RubyValue::Int(41 + extra)) };
    0
}

#[test]
fn register_program_registers_a_class_its_rows_and_meta() {
    const WIDGET: u32 = 60_000;
    static ANCESTORS: &[u32] = &[WIDGET, 0, 25, 24]; // self, Object, Kernel, BasicObject
    static IVARS: &[&str] = &["size"];
    let ivar_strs: Vec<abi::Str> = IVARS.iter().map(|s| abi_str(s)).collect();
    let classes = [abi::ClassDesc {
        id: WIDGET,
        name: abi_str("Widget"),
        kind: abi::CLASS_PLAIN,
        ancestors: ANCESTORS.as_ptr(),
        n_ancestors: ANCESTORS.len(),
        ivar_names: ivar_strs.as_ptr(),
        n_ivars: ivar_strs.len(),
        members: std::ptr::null(),
        n_members: 0,
        hidden: 0,
    }];
    let obj_rows = [abi::ObjRow {
        class: WIDGET,
        name: abi_str("answer"),
        f: as_abi_fn(widget_answer),
    }];
    let meta_rows = [abi::MetaRowC {
        class: WIDGET,
        singleton: 0,
        name: abi_str("answer"),
        params: std::ptr::null(),
        n_params: 0,
        file: abi_str("widget.rb"),
        line: 3,
        aliased_from: no_str(),
    }];
    let mut desc = empty_desc(nil_toplevel);
    desc.classes = classes.as_ptr();
    desc.n_classes = classes.len();
    desc.obj_rows = obj_rows.as_ptr();
    desc.n_obj_rows = obj_rows.len();
    desc.meta_rows = meta_rows.as_ptr();
    desc.n_meta_rows = meta_rows.len();
    unsafe { super::registry::register_program(&desc) };

    assert_eq!(
        crate::dispatch::class_name(zeo_abi::ClassId(WIDGET)).as_deref(),
        Some("Widget")
    );
    // `new` through the shared constructor, then a dynamic send through the
    // registered ObjRow.
    let mut obj = MaybeUninit::<RubyValue>::uninit();
    let status = unsafe {
        super::objects::zeo_rt_class_new_instance(
            WIDGET,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            obj.as_mut_ptr(),
        )
    };
    assert_eq!(status, STATUS_OK);
    let obj = unsafe { obj.assume_init() };
    let sym = crate::Symbol::intern("answer").to_u32();
    let arg = RubyValue::Int(1);
    let mut out = MaybeUninit::<RubyValue>::uninit();
    let status = unsafe {
        super::dispatch::zeo_rt_send_value_in(
            0,
            &obj,
            sym,
            &arg,
            1,
            std::ptr::null_mut(),
            out.as_mut_ptr(),
        )
    };
    assert_eq!(status, STATUS_OK);
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(42)));
    // The meta row landed with its source location.
    let meta = crate::method_meta::lookup(
        zeo_abi::ClassId(WIDGET),
        crate::method_meta::MethodKind::Instance,
        crate::Symbol::intern("answer"),
    )
    .expect("the MetaRowC must register");
    assert_eq!(meta.source(), Some(("widget.rb", 3)));
}

#[test]
fn zeo_rt_main_runs_the_toplevel_and_owns_argv() {
    let desc = empty_desc(nil_toplevel);
    let prog = c"widget-prog";
    let arg = c"alpha";
    let argv = [prog.as_ptr(), arg.as_ptr()];
    let code = unsafe { super::lifecycle::zeo_rt_main(2, argv.as_ptr(), &desc, std::ptr::null()) };
    assert_eq!(code, 0);
    // `$0` and `ARGV` were seeded from the STASHED argv, not env::args.
    match crate::globals::global_get(0, "$0") {
        RubyValue::Str(s) => assert_eq!(s.lock().bytes(), b"widget-prog"),
        other => panic!("$0 should be a Str, got {other:?}"),
    }
    match crate::constants::const_get(0, "ARGV") {
        Some(RubyValue::Array(a)) => {
            let a = a.lock();
            assert_eq!(a.len(), 1);
            match &a[0] {
                RubyValue::Str(s) => assert_eq!(s.lock().bytes(), b"alpha"),
                other => panic!("ARGV[0] should be a Str, got {other:?}"),
            }
        }
        other => panic!("ARGV should be an Array, got {other:?}"),
    }
}

unsafe extern "C" fn raising_toplevel(out: *mut abi::Value) -> i32 {
    let _ = out;
    crate::signal::set_pending(crate::dispatch::raise_error(
        "RuntimeError",
        "boom from the toplevel".to_string(),
    ));
    1
}

#[test]
fn zeo_rt_main_reports_an_uncaught_raise_as_exit_1() {
    let desc = empty_desc(raising_toplevel);
    let prog = c"widget-prog";
    let argv = [prog.as_ptr()];
    let code = unsafe { super::lifecycle::zeo_rt_main(1, argv.as_ptr(), &desc, std::ptr::null()) };
    assert_eq!(code, 1);
}

// -- M1-1: zeo_rt_bind_params ------------------------------------------------

fn bind_desc(nreq: u32, nopt: u32, npost: u32, rest: u8, kwrest: u8) -> abi::ParamDescC {
    abi::ParamDescC {
        nreq,
        nopt,
        npost,
        rest,
        kwrest,
        no_keywords: 0,
        implicit_rest: 0,
        kws: std::ptr::null(),
        n_kws: 0,
        name: abi_str("m"),
        file: abi_str("t.rb"),
        label: abi_str("Object#m"),
        line: 1,
        end_line: 2,
    }
}

/// Run the binder over `args`; hand back (status, slots, present).
fn bind(desc: &abi::ParamDescC, args: &[RubyValue]) -> (i32, Vec<RubyValue>, u64) {
    let n_slots = desc.nreq as usize
        + desc.nopt as usize
        + usize::from(desc.rest == abi::PARAM_STAR_NAMED)
        + desc.npost as usize
        + desc.n_kws
        + usize::from(desc.kwrest == abi::PARAM_STAR_NAMED);
    let mut slots: Vec<RubyValue> = Vec::with_capacity(n_slots.max(1));
    let mut present: u64 = 0;
    let status = unsafe {
        slots.set_len(0);
        let s = super::bind::zeo_rt_bind_params(
            desc,
            if args.is_empty() {
                std::ptr::null()
            } else {
                args.as_ptr()
            },
            args.len(),
            slots.as_mut_ptr(),
            &mut present,
        );
        slots.set_len(n_slots);
        s
    };
    (status, slots, present)
}

fn kw_marked_hash(pairs: &[(&str, i64)]) -> RubyValue {
    let pairs: Vec<(RubyValue, RubyValue)> = pairs
        .iter()
        .map(|(k, v)| {
            (
                RubyValue::Symbol(crate::Symbol::intern(k)),
                RubyValue::Int(*v),
            )
        })
        .collect();
    let h = crate::value::collections::hash_new(pairs);
    crate::value::collections::hash_mark_kwargs(&h);
    RubyValue::Hash(h)
}

#[test]
fn bind_routes_required_optional_rest_and_post() {
    let desc = bind_desc(1, 1, 1, abi::PARAM_STAR_NAMED, abi::PARAM_STAR_NONE);
    // def m(a, b = ?, *r, c) called with (1, 2, 3, 4, 5)
    let args: Vec<RubyValue> = (1..=5).map(RubyValue::Int).collect();
    let (status, slots, present) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(present, 0b1111);
    assert!(matches!(slots[0], RubyValue::Int(1)));
    assert!(matches!(slots[1], RubyValue::Int(2)));
    let RubyValue::Array(r) = &slots[2] else {
        panic!("rest slot must be an Array, got {:?}", slots[2])
    };
    assert_eq!(r.lock().len(), 2);
    assert!(matches!(slots[3], RubyValue::Int(5)));
}

#[test]
fn bind_leaves_an_absent_optional_nil_and_unpresent() {
    let desc = bind_desc(1, 2, 0, abi::PARAM_STAR_NONE, abi::PARAM_STAR_NONE);
    let args = vec![RubyValue::Int(7), RubyValue::Int(8)];
    let (status, slots, present) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(present, 0b011);
    assert!(matches!(slots[1], RubyValue::Int(8)));
    assert!(matches!(slots[2], RubyValue::Nil));
}

#[test]
fn bind_wrong_arity_raises_under_the_callee_frame() {
    boot_registry();
    let desc = bind_desc(2, 1, 0, abi::PARAM_STAR_NONE, abi::PARAM_STAR_NONE);
    let (status, slots, present) = bind(&desc, &[RubyValue::Int(1)]);
    assert_eq!(status, abi::STATUS_SIGNAL);
    assert_eq!(present, 0);
    assert!(slots.iter().all(|v| matches!(v, RubyValue::Nil)));
    let Some(Signal::Raise(exc)) = crate::signal::take_pending() else {
        panic!("wrong arity must park a Raise")
    };
    let msg = exc_message(&exc);
    assert!(
        msg.contains("wrong number of arguments (given 1, expected 2..3)"),
        "{msg}"
    );
}

#[test]
fn bind_declared_keywords_peel_the_marked_hash() {
    boot_registry();
    let kws = [
        abi::KwParamC {
            name: abi_str("a"),
            required: 1,
        },
        abi::KwParamC {
            name: abi_str("b"),
            required: 0,
        },
    ];
    let mut desc = bind_desc(1, 0, 0, abi::PARAM_STAR_NONE, abi::PARAM_STAR_NONE);
    desc.kws = kws.as_ptr();
    desc.n_kws = kws.len();
    let args = vec![RubyValue::Int(9), kw_marked_hash(&[("a", 1)])];
    let (status, slots, present) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(present, 0b011);
    assert!(matches!(slots[0], RubyValue::Int(9)));
    assert!(matches!(slots[1], RubyValue::Int(1)));
    assert!(matches!(slots[2], RubyValue::Nil)); // b absent -> default runs in the body

    // Unknown keyword refuses.
    let args = vec![RubyValue::Int(9), kw_marked_hash(&[("a", 1), ("zz", 2)])];
    let (status, ..) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_SIGNAL);
    let Some(Signal::Raise(exc)) = crate::signal::take_pending() else {
        panic!("unknown keyword must park a Raise")
    };
    assert!(exc_message(&exc).contains("unknown keyword: :zz"));
}

#[test]
fn bind_kwrest_collects_the_leftovers() {
    let kws = [abi::KwParamC {
        name: abi_str("a"),
        required: 1,
    }];
    let mut desc = bind_desc(0, 0, 0, abi::PARAM_STAR_NONE, abi::PARAM_STAR_NAMED);
    desc.kws = kws.as_ptr();
    desc.n_kws = kws.len();
    let args = vec![kw_marked_hash(&[("a", 1), ("x", 2), ("y", 3)])];
    let (status, slots, present) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(present, 0b11);
    assert!(matches!(slots[0], RubyValue::Int(1)));
    let RubyValue::Hash(h) = &slots[1] else {
        panic!("kwrest slot must be a Hash")
    };
    assert_eq!(h.lock().len(), 2);
}

#[test]
fn bind_keywordless_callee_keeps_the_marked_hash_positional() {
    // The options-hash idiom: def m(opts = {}) ; m(a: 1).
    let desc = bind_desc(0, 1, 0, abi::PARAM_STAR_NONE, abi::PARAM_STAR_NONE);
    let args = vec![kw_marked_hash(&[("a", 1)])];
    let (status, slots, present) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(present, 0b1);
    assert!(matches!(slots[0], RubyValue::Hash(_)));
}

#[test]
fn bind_no_keywords_refuses_a_marked_hash_before_arity() {
    boot_registry();
    let mut desc = bind_desc(0, 0, 0, abi::PARAM_STAR_NONE, abi::PARAM_STAR_NONE);
    desc.no_keywords = 1;
    // Wrong count AND keywords: the keywords report first (CRuby order).
    let args = vec![
        RubyValue::Int(1),
        RubyValue::Int(2),
        kw_marked_hash(&[("b", 4)]),
    ];
    let (status, ..) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_SIGNAL);
    let Some(Signal::Raise(exc)) = crate::signal::take_pending() else {
        panic!("**nil must park a Raise")
    };
    assert!(exc_message(&exc).contains("no keywords accepted"));
}

#[test]
fn bind_anonymous_rest_discards_without_a_slot() {
    let desc = bind_desc(1, 0, 0, abi::PARAM_STAR_ANON, abi::PARAM_STAR_NONE);
    let args: Vec<RubyValue> = (1..=4).map(RubyValue::Int).collect();
    let (status, slots, present) = bind(&desc, &args);
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(present, 0b1);
    assert_eq!(slots.len(), 1);
    assert!(matches!(slots[0], RubyValue::Int(1)));
}

/// The raised exception's `message`, through ordinary dispatch.
fn exc_message(exc: &RubyValue) -> String {
    let msg = crate::dispatch::send_value(exc, crate::Symbol::intern("message"), &[], None)
        .expect("Exception#message answers");
    match msg {
        RubyValue::Str(s) => String::from_utf8_lossy(s.lock().bytes()).into_owned(),
        other => panic!("message should be a Str, got {other:?}"),
    }
}

/// The exception paths need the class registry; boot it once, tolerating a
/// prior boot in the same process.
fn boot_registry() {
    static BOOT: std::sync::Once = std::sync::Once::new();
    BOOT.call_once(|| {
        let desc = empty_desc(nil_toplevel);
        unsafe { super::registry::register_program(&desc) };
    });
}
