//! Minimal `Array`/`Hash`/`String` runtime support (Phase 3) -- concrete
//! `Rc<RefCell<_>>`-backed collection types, mirroring the same
//! shared-mutable-identity model `ruby_class!`'s generated structs already
//! use (`RefCell` ivars) and the `Rc<ConcreteStruct>` wrapping `New`
//! constructed objects settled on (see `codegen::call::emit_new`'s docs):
//! assigning a collection to another local aliases the same underlying
//! storage rather than deep-copying it, matching Ruby's own reference
//! semantics for these types.
//!
//! Deliberately NOT a general Enumerable implementation -- see the plan's
//! Phase 3 scope-cut: `[]`/`[]=`/`length` only. `each`/`map`/`select`/etc.
//! are written in Ruby once blocks (Part 1.3) and modules (Part 1.2) exist,
//! not hand-implemented here.

use crate::RubyValue;
use std::cell::RefCell;
use std::rc::Rc;

pub type RArray = Rc<RefCell<Vec<RubyValue>>>;

/// Hash storage is a plain association *list*, not a real hash table: every
/// lookup/insert is an O(n) linear scan compared via `RubyValue::rb_eq`. A
/// real `HashMap` needs `Hash`/`Eq` on `RubyValue`, which in turn needs a
/// user-overridable `#hash`/`#eql?` protocol for `Object` keys that doesn't
/// exist yet -- a deliberate, documented spike scope-cut, not an oversight.
/// Fine for the tiny hashes the spike's examples use; revisit once
/// user-defined `#hash` exists.
pub type RHash = Rc<RefCell<Vec<(RubyValue, RubyValue)>>>;

pub type RStr = Rc<RefCell<String>>;

pub fn array_new(elems: Vec<RubyValue>) -> RArray {
    Rc::new(RefCell::new(elems))
}

/// Ruby's own `Array#[]`: negative indices count from the end, and an
/// out-of-range index returns `nil` rather than raising/panicking.
pub fn array_get(arr: &RArray, index: i64) -> RubyValue {
    let arr = arr.borrow();
    resolve_index(index, arr.len())
        .and_then(|i| arr.get(i).cloned())
        .unwrap_or(RubyValue::Nil)
}

/// Ruby's `Array#[]=`: an index past the current end pads with `nil` up to
/// it (`a = []; a[3] = :x` gives `[nil, nil, nil, :x]`); a negative index
/// that's still out of range panics -- a real `IndexError` needs exceptions
/// (Phase 9), so this is a loud failure, not a silent one, in the meantime.
pub fn array_set(arr: &RArray, index: i64, value: RubyValue) -> RubyValue {
    let mut arr = arr.borrow_mut();
    let i = if index < 0 {
        let from_end = arr.len() as i64 + index;
        usize::try_from(from_end).unwrap_or_else(|_| {
            panic!("index {index} too small for array of length {}", arr.len())
        })
    } else {
        index as usize
    };
    if i >= arr.len() {
        arr.resize(i + 1, RubyValue::Nil);
    }
    arr[i] = value.clone();
    value
}

pub fn array_len(arr: &RArray) -> i64 {
    arr.borrow().len() as i64
}

/// Flattens a `*splat` array-literal element in place -- panics (not a
/// silent no-op) if the splatted value isn't actually an `Array`, since
/// there's no static type-checker here to catch that earlier (same posture
/// as `RubyValue::as_int_unchecked` etc.).
pub fn array_splat_into(out: &mut Vec<RubyValue>, value: &RubyValue) {
    match value {
        RubyValue::Array(a) => out.extend(a.borrow().iter().cloned()),
        other => panic!("expected an Array to splat, got {}", other.to_display_string()),
    }
}

fn resolve_index(index: i64, len: usize) -> Option<usize> {
    let i = if index < 0 { index + len as i64 } else { index };
    usize::try_from(i).ok().filter(|&i| i < len)
}

pub fn hash_new(pairs: Vec<(RubyValue, RubyValue)>) -> RHash {
    let h: RHash = Rc::new(RefCell::new(Vec::new()));
    for (k, v) in pairs {
        hash_set(&h, k, v);
    }
    h
}

/// `Hash#[]`: a missing key returns `nil` (the no-default-block spike
/// scope-cut -- real Ruby's per-instance `Hash.new(default)`/
/// `Hash#default_proc` aren't modeled).
pub fn hash_get(h: &RHash, key: &RubyValue) -> RubyValue {
    h.borrow()
        .iter()
        .find(|(k, _)| k.rb_eq(key))
        .map(|(_, v)| v.clone())
        .unwrap_or(RubyValue::Nil)
}

/// `Hash#[]=`: replaces an existing key's value in place (preserving
/// insertion order, matching real Ruby) rather than appending a duplicate.
pub fn hash_set(h: &RHash, key: RubyValue, value: RubyValue) -> RubyValue {
    let mut h = h.borrow_mut();
    match h.iter_mut().find(|(k, _)| k.rb_eq(&key)) {
        Some((_, v)) => *v = value.clone(),
        None => h.push((key, value.clone())),
    }
    value
}

pub fn hash_len(h: &RHash) -> i64 {
    h.borrow().len() as i64
}

pub fn string_new(s: String) -> RStr {
    Rc::new(RefCell::new(s))
}

/// Character-indexed (not byte-indexed), matching Ruby's own UTF-8-aware
/// `String#[]` -- negative indices count from the end, out-of-range returns
/// `nil`. Re-walking `.chars()` on every call is a real inefficiency for long
/// strings (documented, not fixed -- a byte-offset cache is a
/// straightforward later optimization, not a spike blocker).
pub fn string_get(s: &RStr, index: i64) -> RubyValue {
    let s = s.borrow();
    let chars: Vec<char> = s.chars().collect();
    match resolve_index(index, chars.len()) {
        Some(i) => RubyValue::Str(string_new(chars[i].to_string())),
        None => RubyValue::Nil,
    }
}

/// `String#[]=` with a single-character replacement value (the common case);
/// panics if `value` isn't a `Str` or the index is out of range --
/// multi-character splice-replace (`s[1] = "ab"`) isn't supported yet (spike
/// scope, same posture as `array_set`'s negative-out-of-range panic).
pub fn string_set(s: &RStr, index: i64, value: &RubyValue) -> RubyValue {
    let RubyValue::Str(new_chars) = value else {
        panic!("expected a String, got {}", value.to_display_string());
    };
    let new_chars = new_chars.borrow().clone();
    let mut s = s.borrow_mut();
    let mut chars: Vec<char> = s.chars().collect();
    let i = resolve_index(index, chars.len()).unwrap_or_else(|| {
        panic!("index {index} out of range for string of length {}", chars.len())
    });
    chars.splice(i..=i, new_chars.chars());
    *s = chars.into_iter().collect();
    value.clone()
}

pub fn string_len(s: &RStr) -> i64 {
    s.borrow().chars().count() as i64
}
