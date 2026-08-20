//! `for x in coll` -- the loop the emitter drives one step at a time.
//!
//! Ruby's `for` performs no type dispatch: `compile_iter` emits
//! `obj.each { |x| .. }`, and an iterable answering no `each` raises
//! `NoMethodError` at run time. The Array and counted-Range modes below
//! are fast paths over that -- and the Array one is a SEMANTIC path, not
//! only a fast one: CRuby re-reads the length and the current element
//! every step, so an append during iteration is visited and a `pop`
//! shortens the walk.

use crate::RubyValue;
use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

/// A live Array: index into the receiver itself, re-reading its length.
const KIND_LIVE: u8 = 0;
/// An Int-bounded Range: count natively, no allocation.
const KIND_COUNTED: u8 = 1;
/// Everything else: the values `each` yielded, collected up front (the
/// snapshot the labelled `break`/`next`/`redo` shape relies on).
const KIND_ITEMS: u8 = 2;

/// The emitter's opaque loop state, held in one stack slot. `zeo_rt_for_begin`
/// fills it and `zeo_rt_for_end` releases what it holds -- a matched pair, on
/// the normal exit and on the error landing alike.
#[repr(C)]
pub struct ForState {
    coll: RubyValue,
    items: RubyValue,
    idx: usize,
    i: i64,
    end: i64,
    kind: u8,
    exclusive: u8,
    endless: u8,
    started: u8,
}

const _: () = assert!(std::mem::size_of::<ForState>() <= zeo_abi::abi::FOR_STATE_SIZE);

/// Open a `for` loop over `coll`. `packed` is `ForBind`: `for x in obj`
/// binds a multi-value yield's FIRST value, `for k, v in obj` packs.
/// Fallible -- an iterable with no `each` raises here, as ruby's does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_for_begin(
    coll: *const RubyValue,
    packed: u8,
    state: *mut ForState,
) -> i32 {
    let coll = unsafe { &*coll }.clone();
    let counted = matches!(&coll, RubyValue::Range(_)) && {
        let begin = coll.range_first();
        let end = coll.range_last();
        matches!(begin, RubyValue::Int(_)) && matches!(end, RubyValue::Int(_) | RubyValue::Nil)
    };
    let (kind, mut i, mut end, mut exclusive, mut endless) = if matches!(coll, RubyValue::Array(_))
    {
        (KIND_LIVE, 0, 0, 0, 0)
    } else if counted {
        let end_v = coll.range_last();
        let endless = u8::from(matches!(end_v, RubyValue::Nil));
        (
            KIND_COUNTED,
            coll.range_first().as_int_unchecked(),
            if endless == 1 {
                0
            } else {
                end_v.as_int_unchecked()
            },
            u8::from(coll.range_exclude_end()),
            endless,
        )
    } else {
        (KIND_ITEMS, 0, 0, 0, 0)
    };
    let bind = if packed == 0 {
        crate::builtins::enumerable::ForBind::First
    } else {
        crate::builtins::enumerable::ForBind::Packed
    };
    let items = if kind == KIND_ITEMS {
        // `each_values` raises for a beginless or Float range exactly
        // where ruby's `Range#each` does.
        match crate::builtins::enumerable::each_values(&coll, bind) {
            Ok(v) => RubyValue::Array(crate::value::collections::array_new(v)),
            Err(sig) => {
                crate::signal::set_pending(sig);
                return STATUS_SIGNAL;
            }
        }
    } else {
        RubyValue::Nil
    };
    if kind != KIND_COUNTED {
        i = 0;
        end = 0;
        exclusive = 0;
        endless = 0;
    }
    unsafe {
        state.write(ForState {
            coll,
            items,
            idx: 0,
            i,
            end,
            kind,
            exclusive,
            endless,
            started: 0,
        });
    }
    STATUS_OK
}

/// One step: writes the next element and `more = 1`, or `more = 0` when
/// the walk is over. The step is at the TOP of the loop so a `next`
/// (which jumps to the head) still advances.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_for_next(state: *mut ForState, out: *mut RubyValue, more: *mut u8) {
    let s = unsafe { &mut *state };
    if s.started == 1 {
        match s.kind {
            KIND_COUNTED => s.i += 1,
            _ => s.idx += 1,
        }
    }
    s.started = 1;
    let elem = match s.kind {
        KIND_COUNTED => {
            let in_range = s.endless == 1
                || if s.exclusive == 1 {
                    s.i < s.end
                } else {
                    s.i <= s.end
                };
            in_range.then(|| RubyValue::Int(s.i))
        }
        KIND_LIVE => s.coll.as_array_ref().lock().get(s.idx).cloned(),
        _ => s.items.as_array_ref().lock().get(s.idx).cloned(),
    };
    match elem {
        Some(v) => {
            super::leakcheck::created(&v);
            unsafe {
                out.write(v);
                more.write(1);
            }
        }
        None => unsafe { more.write(0) },
    }
}

/// The loop's VALUE: `for x in c; end` answers `c`, the same object
/// (a `break <v>` in the body supplies its own).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_for_result(state: *const ForState, out: *mut RubyValue) {
    let v = unsafe { &*state }.coll.clone();
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Release what [`zeo_rt_for_begin`] put in the state. Called on the
/// normal exit and on the error landing alike.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_for_end(state: *mut ForState) {
    unsafe { std::ptr::drop_in_place(state) };
}
