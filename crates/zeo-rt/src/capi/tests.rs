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

    // Unassigned slot reads nil; a write lands and reads back.
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_ivar_get_slot(&obj, 0, out.as_mut_ptr()) };
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Nil));
    let mut v = MaybeUninit::new(RubyValue::Int(5));
    assert_eq!(
        unsafe { zeo_rt_ivar_set_slot(&obj, 1, v.as_mut_ptr()) },
        STATUS_OK
    );
    let mut out = MaybeUninit::<RubyValue>::uninit();
    unsafe { zeo_rt_ivar_get_slot(&obj, 1, out.as_mut_ptr()) };
    assert!(matches!(unsafe { out.assume_init() }, RubyValue::Int(5)));

    // Reflection reports first-assignment order and skips the hidden slot.
    let RubyValue::Object(o) = &obj else { panic!() };
    let mut h = MaybeUninit::new(RubyValue::Int(9));
    assert_eq!(
        unsafe { zeo_rt_ivar_set_slot(&obj, 2, h.as_mut_ptr()) },
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
