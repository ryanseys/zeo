//! `Array` (CRuby array.c) -- stage B carries the rows migrated from the
//! old curated table (arg-type mismatches upgraded from silent fall-through
//! to CRuby's real TypeError); the Tier A breadth lands in stage E.

use crate::RubyValue;
use crate::builtins::enumerable::{self, own_row};
use crate::builtins::{
    arg_error, arg_int, block_or_enum, convert, index_error, inherited_row, type_error,
};
use zeo_macros::ruby_class;

/// `arr[1..3]` -- the ordinary Range slice: an out-of-range begin answers
/// nil, and an end past the last element clamps rather than raising.
fn range_slice(
    rary: &crate::RArray,
    start: Option<&RubyValue>,
    end: Option<&RubyValue>,
    exclusive: bool,
) -> Result<RubyValue, crate::Signal> {
    // CRuby's `rb_range_beg_len` converts BOTH endpoints with `NUM2LONG`
    // before it asks anything about the array, so an endpoint that is not an
    // index is an ERROR rather than a miss: a bignum raises RangeError, a
    // String raises TypeError, and a Float truncates. `nil` answers the
    // array's own edge -- a beginless or endless range, not a conversion.
    let bound = |v: Option<&RubyValue>| match v {
        None | Some(RubyValue::Nil) => Ok(None),
        Some(v) => convert::to_index(v).map(Some),
    };
    let s = bound(start)?;
    let e = bound(end)?;
    let guard = rary.lock();
    let n = guard.len() as i64;
    let s = match s {
        Some(v) if v < 0 => v + n,
        Some(v) => v,
        None => 0,
    };
    let e = match e {
        Some(v) => {
            let v = if v < 0 { v + n } else { v };
            if exclusive { v - 1 } else { v }
        }
        None => n - 1,
    };
    if s < 0 || s > n {
        return Ok(RubyValue::Nil);
    }
    let e = e.min(n - 1);
    Ok(RubyValue::Array(crate::array_new(if e < s {
        Vec::new()
    } else {
        guard[s as usize..=e as usize].to_vec()
    })))
}

/// An arithmetic sequence's endpoint or step, as an array index. CRuby's
/// `NUM2LONG` truncates toward zero, which is why `(1.5..5.5).step(2)`
/// slices from 1 through 5.
fn seq_index(v: &RubyValue) -> i64 {
    match v {
        RubyValue::Int(n) => *n,
        // `as i64` saturates, and any index that far out is out of range
        // either way.
        other => crate::builtins::numeric::num_to_f64_unchecked(other) as i64,
    }
}

/// `arr[(1..10).step(2)]` -- CRuby's `rb_arithmetic_sequence_beg_len_step`
/// reduced to the span `(begin, len)` plus a stride.
///
/// A sequence is stricter than the equivalent Range: where `arr[20..30]`
/// answers nil, `arr[(20..30).step(2)]` raises. The exception is a step of
/// exactly 1, which CRuby routes through the ordinary Range slice instead --
/// so `arr[(20..30).step(1)]` IS nil.
fn arith_seq_slice(rary: &crate::RArray, seq: &RubyValue) -> Result<RubyValue, crate::Signal> {
    let (begin, end, step, exclude_end) = crate::builtins::enumerator::arith_seq_parts(seq)
        .expect("the caller checked this is an arithmetic sequence");
    let stride = seq_index(step);
    if stride == 0 {
        return Err(arg_error!("slice step cannot be zero"));
    }
    // A beginless sequence starts at the array's own beginning.
    let begin = match begin {
        RubyValue::Nil => None,
        v => Some(v),
    };
    if stride == 1 {
        return range_slice(rary, begin, end, exclude_end);
    }
    // A descending sequence walks the SAME span from its far end, so the
    // endpoints swap -- and an exclusive end, now the span's low side,
    // excludes the first index rather than the last.
    let (low, high, mut excl) = if stride < 0 {
        (end, begin, false)
    } else {
        (begin, end, exclude_end)
    };
    let bump = (stride < 0 && exclude_end && low.is_some()) as i64;
    let n = rary.lock().len() as i64;
    let mut b = low.map_or(0, seq_index) + bump;
    let mut e = match high {
        Some(v) => seq_index(v),
        None => {
            excl = false;
            -1
        }
    };
    let out_of_range = || crate::builtins::range_error!("{} out of range", seq.inspect_string());
    if b < 0 {
        b += n;
        if b < 0 {
            return Err(out_of_range());
        }
    }
    if e < 0 {
        e += n;
    }
    if !excl {
        e += 1;
    }
    let len = (e - b).max(0);
    if b > n || len > n {
        return Err(out_of_range());
    }
    let guard = rary.lock();
    let mut out = Vec::new();
    if stride > 0 {
        let mut i = b;
        while i < (b + len).min(n) {
            out.push(guard[i as usize].clone());
            i += stride;
        }
    } else if len > 0 {
        let mut i = (b + len - 1).min(n - 1);
        while i >= b {
            out.push(guard[i as usize].clone());
            i += stride;
        }
    }
    Ok(RubyValue::Array(crate::array_new(out)))
}

ruby_class! {
    Array = zeo_abi::ARRAY_CLASS < zeo_abi::OBJECT_CLASS;
    receiver rary = crate::RubyValue::Array;
    include zeo_abi::ENUMERABLE_CLASS;

    // `Array[1, 2, 3]` -- the literal-like constructor, which takes its
    // arguments as the elements rather than as a size and a fill.
    def self."[]" allocs (_recv, *items, &_block) {
        Ok(RubyValue::Array(crate::array_new(items.to_vec())))
    }

    // `Array.new(size = 0, default = nil)` / `Array.new(size) { |i| ... }`.
    // Reached through `class_method_table` on a `RubyValue::Class` receiver
    // -- `Array` has no generated struct, so there is no constructor for
    // `Class#new`'s ordinary allocator path to find.
    //
    // The default-value form SHARES one object across every slot (real
    // Ruby: `a = Array.new(2, "x"); a[0] << "!"` changes `a[1]` too), which
    // is exactly why the block form exists; a `RubyValue` clone is a handle
    // clone, so that sharing is inherited rather than needing to be built.
    // `Array.try_convert(obj)`: `obj` if it's already an Array, its `to_ary`
    // if it defines one (which must yield an Array or nil), else nil. Unlike
    // `Array(obj)` it never wraps or raises for a non-convertible value.
    // Answers the object it was handed, so a `class L < Array` stays an `L`.
    def self."try_convert" (_recv, arg) {
        Ok(convert::try_convert_value(arg, "Array", "to_ary")?.unwrap_or(RubyValue::Nil))
    }
    def self."new" allocs (_recv, size?, fill?, &block) {
        // `Array.new(other_array)` is the COPY form (CRuby `rb_ary_initialize`):
        // a shallow copy of the given array, ignoring any block. Only when the
        // sole argument is an Array -- otherwise the arg is a size below.
        if fill.is_none()
            && let Some(RubyValue::Array(a)) = size {
                return Ok(RubyValue::Array(crate::array_new(a.lock().to_vec())));
            }
        if block.is_some() && fill.is_some() {
            crate::builtins::warning::rb_warn("block supersedes default value argument");
        }
        let size = match size {
            None => 0,
            Some(v) => arg_int!(v),
        };
        if size < 0 {
            return Err(arg_error!("negative array size"));
        }
        let size = size as usize;
        if let Some(RubyValue::Proc(p)) = &block {
            let mut out = Vec::with_capacity(size);
            for i in 0..size {
                out.push(p.call(&[RubyValue::Int(i as i64)])?);
            }
            return Ok(RubyValue::Array(crate::array_new(out)));
        }
        let fill = fill.cloned().unwrap_or(RubyValue::Nil);
        Ok(RubyValue::Array(crate::array_new(vec![fill; size])))
    }

    def "[]" | "slice" cfunc (recv, index, len?) {
        // NO up-front whole-Vec snapshot: a plain `arr[i]` in a loop must be
        // O(1), not O(n) (bm_huffman spent 250s cloning arrays here). The
        // two slice shapes copy only the requested span, under the lock; any
        // dispatch-capable conversion (`to_int` ducks) runs BEFORE locking,
        // so user code can never re-enter this array while it is held.
        // `arr[start, len]`.
        if let Some(len) = len {
            let (start, len) = (arg_int!(index), arg_int!(len));
            let guard = rary.lock();
            let n = guard.len() as i64;
            let start = if start < 0 { start + n } else { start };
            if start < 0 || start > n || len < 0 {
                return Ok(RubyValue::Nil);
            }
            // Saturating: `len` comes straight from user code, and release
            // builds have overflow-checks off, so a plain `start + len` with
            // `len` near `i64::MAX` wraps NEGATIVE -- the `.min(n)` clamp then
            // reads false and the slice index panics. ruby clamps to the end.
            let end = start.saturating_add(len).min(n);
            return Ok(RubyValue::Array(crate::array_new(
                guard[start as usize..end as usize].to_vec(),
            )));
        }
        match index {
            // The plain-Int index FIRST: it is the hot shape, and putting it
            // ahead of the arith-seq guard keeps that probe (a class walk)
            // off every `arr[i]`.
            RubyValue::Int(i) => Ok(crate::array_get(rary, *i)),
            // `arr[1..3]` -- Range slicing.
            RubyValue::Range(__rg) => {
                let (start, end, exclusive) = __rg.parts();
                range_slice(rary, start, end, exclusive)
            }
            // `arr[(1..10).step(2)]` -- an arithmetic sequence slices with a
            // STRIDE (CRuby's `rb_arithmetic_sequence_beg_len_step`).
            other if crate::builtins::enumerator::arith_seq_parts(other).is_some() => {
                arith_seq_slice(rary, other)
            }
            other => {
                let i = convert::to_index(other)?;
                Ok(crate::array_get(rary, i))
            }
        }
    }
    def "[]=" cfunc (recv, index, second, third?) {
        // The frozen check comes FIRST -- before length/index validation --
        // matching CRuby's `rb_ary_modify_check` at the top of the mutator
        // (oracle-verified ordering: FrozenError wins over a negative
        // length, a too-small index, and an out-of-range range alike).
        check_frozen(rary, recv)?;
        // `arr[start, len] = val` / `arr[range] = val` -- CRuby's splice
        // (rb_ary_splice): the removed span is replaced by the VALUE's
        // `to_ary` coercion's elements (a plain Array as-is; an object
        // without `to_ary` inserts as one element). The expression value is
        // the object as written, never the coercion.
        if let Some(value) = third {
            let (start, len) = (arg_int!(index), arg_int!(second));
            array_splice(rary, start, len, value)?;
            return Ok(value.clone());
        }
        if let RubyValue::Range(__rg) = index {
            let (s, e, exclusive) = __rg.parts();
            let n = crate::array_len(rary);
            let start = match s {
                Some(v) => {
                    let v = convert::to_index(v)?;
                    if v < 0 { v + n } else { v }
                }
                None => 0,
            };
            // A range whose begin lands before the front is a RangeError
            // (`rb_range_beg_len`'s err path), NOT the scalar forms'
            // IndexError -- rendered with the range's own inspect.
            if start < 0 {
                return Err(crate::builtins::range_error!(
                    "{} out of range",
                    index.inspect_string()
                ));
            }
            let end = match e {
                Some(v) => {
                    let v = convert::to_index(v)?;
                    if v < 0 { v + n } else { v }
                }
                None => n - 1,
            };
            let len = (end - start + if exclusive { 0 } else { 1 }).max(0);
            array_splice(rary, start, len, second)?;
            return Ok(second.clone());
        }
        let i = arg_int!(index);
        if crate::array_set(rary, i, second.clone()) {
            Ok(second.clone())
        } else {
            // `minimum:` is the NUMBER `-len`, so an empty array reads
            // `minimum: 0` -- not a literal minus in front of the length,
            // which spelled it `-0` (CRuby's `rb_ary_store`).
            Err(index_error!(
                "index {i} too small for array; minimum: {}",
                -(crate::array_len(rary) as i64)
            ))
        }
    }
    def "<<" arity 1 | "push" | "append"(recv, *args, &_block) {
        // `push` is variadic (0+ args) in real Ruby; `<<` is arity 1, but
        // the parser only ever emits it with one argument, so one row
        // serves both.
        let arr = rary;
        check_frozen(arr, recv)?;
        // ONE lock for the whole argument list, and no boxed per-element
        // return: `array_push` re-locked and built a throwaway
        // `RubyValue::Array` per element. Clones under the lock are Arc
        // bumps only -- no user code can run.
        let mut g = arr.lock();
        for a in args {
            g.push(a.clone());
        }
        drop(g);
        Ok(recv.clone())
    }
    def "length" | "size" (recv) {
        Ok(RubyValue::Int(crate::array_len(rary)))
    }
    // `member?` is deliberately NOT an alias here: ruby owns `include?` on
    // Array and `member?` only on Enumerable, so declaring both would put
    // `member?` in `Array.instance_methods(false)` and report its `.owner` as
    // Array. It reaches Enumerable's generic row through the ancestry instead.
    def "include?" (recv, arg) {
        Ok(RubyValue::Bool(crate::array_include(rary, arg)?))
    }
    def "empty?" (recv) {
        Ok(RubyValue::Bool(crate::array_len(rary) == 0))
    }
    // The count is bound by the DSL, so an extra argument is CRuby's own
    // ArgumentError rather than a silently dropped one.
    def "first" params "n = nil"(recv, n?) {
        // `first(n)` is Enumerable's n-form (next ancestor in the chain
        // implements it) -- only the 0-arg head accessor lives here. A negative
        // count is caught here so it carries Array's own message ("negative
        // array size"), distinct from Enumerable's generic one.
        if let Some(arg) = n {
            let n = convert::to_index(arg)?;
            if n < 0 {
                return Err(arg_error!("negative array size"));
            }
            return crate::builtins::enumerable::enumerable_send(
                recv, "first", &[RubyValue::Int(n)], None,
            )
            .expect("Enumerable implements first(n)");
        }
        Ok(crate::array_get(rary, 0))
    }
    // `last`/`last(n)` mirror `pop`'s dual return: bare answers ONE element
    // (nil when empty), `last(n)` an ARRAY of up to the last n, in original
    // order (`n` past the length takes what's there; `n == 0` is `[]`).
    def "last" params "n = nil"(recv, n?) {
        let items = rary.lock();
        let Some(n) = count_arg(n)? else {
            return Ok(items.last().cloned().unwrap_or(RubyValue::Nil));
        };
        let at = items.len().saturating_sub(n);
        Ok(RubyValue::Array(crate::array_new(items[at..].to_vec())))
    }
    def "to_a" (recv) {
        Ok(recv.clone())
    }
    def "+" (recv, other) {
        let other = &convert::to_rary(other)?;
        let mut out = rary.lock().to_vec();
        out.extend(other.lock().iter().cloned());
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "-" (recv, other) {
        let other = &convert::to_rary(other)?;
        let exclude = other.lock().clone();
        let out = rary
            .lock()
            .iter()
            .filter(|e| !exclude.iter().any(|x| e.rb_eq(x)))
            .cloned()
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `arr * n` repeats; `arr * "sep"` joins (real Ruby's dual form --
    // CRuby probes `to_str` first, then falls through to the count).
    def "*" (recv, other) {
        if let Some(sep) = convert::check_to_str(other)? {
            return join(recv, &[sep], None);
        }
        let n = convert::to_index(other)?;
        if n < 0 {
            return Err(arg_error!("negative argument"));
        }
        let base = rary.lock().clone();
        let mut out = Vec::with_capacity(base.len() * n as usize);
        for _ in 0..n {
            out.extend(base.iter().cloned());
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "&" (recv, other) {
        let other = &convert::to_rary(other)?;
        let keep = other.lock().clone();
        let mut out: Vec<RubyValue> = Vec::new();
        for e in rary.lock().iter() {
            if keep.iter().any(|x| e.rb_eq(x)) && !out.iter().any(|x| e.rb_eq(x)) {
                out.push(e.clone());
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // Variadic siblings of `&`/`-`: `intersection` keeps self's elements
    // present in EVERY argument (uniq'd); `difference` keeps self's elements
    // absent from ALL arguments (duplicates preserved, like `-`).
    def "intersection"(recv, *args, &_block) {
        let others = set_op_args(args)?;
        let mut out: Vec<RubyValue> = Vec::new();
        for e in rary.lock().iter() {
            let in_all = others.iter().all(|o| o.iter().any(|x| e.rb_eq(x)));
            if in_all && !out.iter().any(|x| e.rb_eq(x)) {
                out.push(e.clone());
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "difference"(recv, *args, &_block) {
        let others = set_op_args(args)?;
        let out: Vec<RubyValue> = rary
            .lock()
            .iter()
            .filter(|e| !others.iter().any(|o| o.iter().any(|x| e.rb_eq(x))))
            .cloned()
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `|` is the BINARY operator (`a | b`); `union` is its variadic sibling
    // (`a.union(b, c, ...)`, zero args = a uniq'd copy of self). Both drop
    // later duplicates, keeping first-occurrence order.
    def "|" (recv, other) {
        let others = set_op_args(std::slice::from_ref(other))?;
        Ok(RubyValue::Array(crate::array_new(union_of(rary, &others))))
    }
    def "union"(recv, *args, &_block) {
        let others = set_op_args(args)?;
        Ok(RubyValue::Array(crate::array_new(union_of(rary, &others))))
    }
    def "<=>" (recv, other) {
        let RubyValue::Array(other) = other else {
            return Ok(RubyValue::Nil);
        };
        let me = rary;
        // Comparing an array to itself (including a self-referential one) is 0
        // -- and short-circuiting avoids both a self-deadlock on the shared
        // mutex and unbounded recursion into a cyclic element.
        if std::sync::Arc::ptr_eq(me, other) {
            return Ok(RubyValue::Int(0));
        }
        // Snapshot each side in its own statement so the first lock is released
        // before the second is taken (the two could still alias deeper).
        let a = me.lock().clone();
        let b = other.lock().clone();
        for (x, y) in a.iter().zip(b.iter()) {
            match x.rb_cmp(y) {
                Some(0) => continue,
                Some(c) => return Ok(RubyValue::Int(c)),
                None => return Ok(RubyValue::Nil),
            }
        }
        Ok(RubyValue::Int((a.len() as i64 - b.len() as i64).signum()))
    }
    def "==" (recv, other) {
        Ok(RubyValue::Bool(recv.rb_eq(other)))
    }
    // `Array#eql?` -- like `==` but per element with `eql?` (class-strict:
    // `1.eql?(1.0)` is false, unlike `==`).
    def "eql?" (recv, arg) {
        let RubyValue::Array(other) = arg else {
            return Ok(RubyValue::Bool(false));
        };
        let me = rary;
        if std::sync::Arc::ptr_eq(me, other) {
            return Ok(RubyValue::Bool(true));
        }
        // Sequential snapshots -- a tuple `(x.lock().., y.lock()..)` keeps
        // BOTH guards alive to the end of the statement, and two threads
        // running `a.eql?(b)` / `b.eql?(a)` in parallel would deadlock on
        // the opposite lock orders.
        let a = me.lock().clone();
        let b = other.lock().clone();
        let eq = a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_eql(x, y));
        Ok(RubyValue::Bool(eq))
    }
    // `pop`/`shift` answer ONE element (nil when empty); `pop(n)`/`shift(n)`
    // answer an ARRAY of up to n -- a different return type, not just a
    // different count, which is why the no-arg case can't just be `pop(1)`.
    // `n` past the length takes what's there; `n == 0` is `[]`.
    def "pop"(recv, n?) {
        let handle = rary;
        check_frozen(handle, recv)?;
        let mut guard = handle.lock();
        let Some(n) = count_arg(n)? else {
            return Ok(guard.pop().unwrap_or(RubyValue::Nil));
        };
        let at = guard.len().saturating_sub(n);
        let taken: Vec<RubyValue> = guard.split_off(at);
        Ok(RubyValue::Array(crate::array_new(taken)))
    }
    def "shift"(recv, n?) {
        let handle = rary;
        check_frozen(handle, recv)?;
        let mut guard = handle.lock();
        let Some(n) = count_arg(n)? else {
            return Ok(guard.shift().unwrap_or(RubyValue::Nil));
        };
        let n = n.min(guard.len());
        let rest = guard.split_off(n);
        let taken = std::mem::replace(&mut *guard, rest.into());
        Ok(RubyValue::Array(crate::array_new(taken.into_vec())))
    }
    def "unshift" | "prepend"(recv, *args, &_block) {
        let handle = rary;
        check_frozen(handle, recv)?;
        let mut guard = handle.lock();
        for (i, a) in args.iter().enumerate() {
            guard.insert(i, a.clone());
        }
        drop(guard);
        Ok(recv.clone())
    }
    def "concat"(recv, *args, &_block) {
        check_frozen(rary, recv)?;
        // Snapshot every source BEFORE appending: an argument may alias the
        // receiver (`a.concat(a, a)`), and CRuby copies all sources up front,
        // so the growing receiver never feeds itself (which would grow forever
        // -- `[1,2].concat(a,a)` is 6 elements, not 8).
        let mut extension = Vec::new();
        for a in args {
            let other = &convert::to_rary(a)?;
            extension.extend(other.lock().clone());
        }
        rary.lock().extend(extension);
        Ok(recv.clone())
    }
    // `flatten` / `flatten(depth)`.
    def "flatten"(recv, depth?) {
        let depth = match depth {
            None | Some(RubyValue::Nil) => -1,
            Some(v) => arg_int!(v),
        };
        let items = rary.lock().clone();
        Ok(RubyValue::Array(crate::array_new(flatten_to_depth(
            &items, depth,
        )?)))
    }
    def "compact" (recv) {
        let out = rary
            .lock()
            .iter()
            .filter(|e| !e.is_nil())
            .cloned()
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "uniq" (recv, &block) {
        let items = rary.lock().clone();
        let out = uniq_dedup(&items, block.as_ref())?;
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "uniq!" (recv, &block) {
        let h = rary;
        check_frozen(h, recv)?;
        let items = h.lock().clone();
        let out = uniq_dedup(&items, block.as_ref())?;
        if out.len() == items.len() {
            return Ok(RubyValue::Nil); // no change -- CRuby's nil answer
        }
        *h.lock() = out.into();
        Ok(recv.clone())
    }
    def "assoc" (recv, arg) {
        // Snapshot before locking any element: an element can BE the receiver
        // (`a = [1]; a << a`), and taking `inner`'s guard while the receiver's
        // is still held deadlocks a non-reentrant Mutex. `product` below has
        // taken this shape all along.
        for e in crate::collections::array_snapshot(rary) {
            if let RubyValue::Array(inner) = &e
                && inner.lock().first().is_some_and(|k| k.rb_eq(arg)) {
                    return Ok(e);
                }
        }
        Ok(RubyValue::Nil)
    }
    def "rassoc" (recv, arg) {
        for e in crate::collections::array_snapshot(rary) {
            if let RubyValue::Array(inner) = &e
                && inner.lock().get(1).is_some_and(|v| v.rb_eq(arg)) {
                    return Ok(e);
                }
        }
        Ok(RubyValue::Nil)
    }
    def "product"(recv, *args, &block) {
        // Cartesian product of self with every argument array, CRuby's
        // element order (leftmost varies slowest).
        let mut lists: Vec<Vec<RubyValue>> = vec![rary.lock().to_vec()];
        for a in args {
            let other = &convert::to_rary(a)?;
            lists.push(other.lock().to_vec());
        }
        let mut out: Vec<RubyValue> = vec![RubyValue::Array(crate::array_new(Vec::new()))];
        let mut tuples: Vec<Vec<RubyValue>> = vec![Vec::new()];
        for list in &lists {
            let mut next = Vec::with_capacity(tuples.len() * list.len());
            for t in &tuples {
                for e in list {
                    let mut t2 = t.clone();
                    t2.push(e.clone());
                    next.push(t2);
                }
            }
            tuples = next;
        }
        out.clear();
        out.extend(tuples.into_iter().map(|t| RubyValue::Array(crate::array_new(t))));
        // The block form yields each tuple and answers self; blockless returns
        // the product array.
        if let Some(RubyValue::Proc(p)) = &block {
            for tuple in &out {
                p.call(std::slice::from_ref(tuple))?;
            }
            return Ok(recv.clone());
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "transpose" (recv) {
        let rows = rary.lock().clone();
        if rows.is_empty() {
            return Ok(RubyValue::Array(crate::array_new(Vec::new())));
        }
        let mut cols: Vec<Vec<RubyValue>> = Vec::new();
        for (ri, row) in rows.iter().enumerate() {
            let r = &convert::to_rary(row)?;
            let r = r.lock().clone();
            if ri == 0 {
                cols = vec![Vec::with_capacity(rows.len()); r.len()];
            } else if r.len() != cols.len() {
                return Err(index_error!("element size differs ({} should be {})", r.len(), cols.len()));
            }
            for (ci, v) in r.into_iter().enumerate() {
                cols[ci].push(v);
            }
        }
        Ok(RubyValue::Array(crate::array_new(
            cols.into_iter().map(|c| RubyValue::Array(crate::array_new(c))).collect(),
        )))
    }
    def "slice!" cfunc (recv, _start, _length?, &_block) {
        let args = __args;
        // `slice!(i)` / `slice!(i, len)` / `slice!(start..end)` -- remove and
        // return the removed span.
        let h = rary;
        check_frozen(h, recv)?;
        let len = h.lock().len() as i64;
        // Range form: remove and return the sub-array (nil if the start is
        // past the end).
        if let RubyValue::Range(__rg) = &args[0] {
            let (s, e, exclusive) = __rg.parts();
            let start = match s {
                Some(RubyValue::Int(v)) => if *v < 0 { v + len } else { *v },
                None => 0,
                _ => return Ok(RubyValue::Nil),
            };
            if start < 0 || start > len {
                return Ok(RubyValue::Nil);
            }
            let end = match e {
                Some(RubyValue::Int(v)) => {
                    let v = if *v < 0 { v + len } else { *v };
                    if exclusive { v } else { v + 1 }
                }
                None => len,
                _ => return Ok(RubyValue::Nil),
            };
            let end = end.clamp(start, len) as usize;
            let removed: Vec<RubyValue> = h.lock().drain(start as usize..end).collect();
            return Ok(RubyValue::Array(crate::array_new(removed)));
        }
        let i = arg_int!(args, 0);
        let idx = if i < 0 { i + len } else { i };
        if idx < 0 || idx > len {
            return Ok(RubyValue::Nil);
        }
        match args.len() {
            1 => {
                if idx == len {
                    return Ok(RubyValue::Nil);
                }
                Ok(h.lock().remove(idx as usize))
            }
            2 => {
                // A NEGATIVE length is nil, not an empty slice: `rb_ary_splice`
                // rejects it before removing anything, so `a.slice!(1, -1)`
                // answers nil and leaves the array alone.
                let n = arg_int!(args, 1);
                if n < 0 {
                    return Ok(RubyValue::Nil);
                }
                let end = ((idx + n) as usize).min(len as usize);
                let removed: Vec<RubyValue> = h.lock().drain(idx as usize..end).collect();
                Ok(RubyValue::Array(crate::array_new(removed)))
            }
            n => unreachable!("the header admits 1..2 arguments, not {n}"),
        }
    }
    def "reverse" (recv) {
        let mut out = rary.lock().to_vec();
        out.reverse();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "reverse!" (recv) {
        check_frozen(rary, recv)?;
        rary.lock().reverse();
        Ok(recv.clone())
    }
    def "join" as join (recv, arg?) {
        // BYTE concatenation under the encoding-compatibility rule, the same
        // way `String#+` does it -- never through the lossy display text,
        // which replaced every non-UTF-8 byte with U+FFFD and so could not
        // rejoin the pieces of a compressed stream (or anything else binary).
        // An absent separator falls back to `$,`, the output field
        // separator -- ruby reads it at the CALL, so a program that sets it
        // around one `join` gets it for that call only.
        let sep = match arg {
            None | Some(RubyValue::Nil) => crate::builtins::kernel::output_separator("$,"),
            Some(other) => Some(convert::to_rstr(other)?.lock().clone()),
        };
        let elems = rary.lock().clone();
        let mut out = crate::string_new(String::new()).lock().clone();
        join_into(&mut out, &elems, sep.as_ref())?;
        Ok(RubyValue::Str(crate::collections::string_wrap(out)))
    }
    def "index" | "find_index" cfunc (recv, arg?, &block) {
        // Live per-element probes: neither the block nor `rb_eq` (a user
        // `==`) ever runs under the receiver's lock.
        let arr = rary;
        if let Some(RubyValue::Proc(p)) = &block {
            let mut i = 0usize;
            loop {
                let e = {
                    let guard = arr.lock();
                    match guard.get(i) {
                        Some(e) => e.clone(),
                        None => return Ok(RubyValue::Nil),
                    }
                };
                if p.call(&[e])?.truthy() {
                    return Ok(RubyValue::Int(i as i64));
                }
                i += 1;
            }
        }
        // Without a block the value is required -- the block IS the test.
        let Some(needle) = arg else {
            return Err(crate::builtins::arity_err(0, 1, Some(1)));
        };
        let mut i = 0usize;
        loop {
            let e = {
                let guard = arr.lock();
                match guard.get(i) {
                    Some(e) => e.clone(),
                    None => return Ok(RubyValue::Nil),
                }
            };
            if crate::builtins::basic_object::rb_equal(&e, needle)? {
                return Ok(RubyValue::Int(i as i64));
            }
            i += 1;
        }
    }
    // `rindex(obj)` matches by `==` from the right; `rindex { |e| }` finds
    // the last element the block answers truthy for.
    def "rindex" cfunc (recv, arg?, &block) {
        // Right-to-left with per-element lock round-trips (an index the
        // block/`rb_eq` shrank away just skips); neither re-entrant call
        // ever runs under the receiver's lock.
        let arr = rary;
        let len = arr.lock().len();
        if let Some(RubyValue::Proc(p)) = &block {
            for i in (0..len).rev() {
                let e = match arr.lock().get(i).cloned() {
                    Some(e) => e,
                    None => continue,
                };
                if p.call(&[e])?.truthy() {
                    return Ok(RubyValue::Int(i as i64));
                }
            }
            return Ok(RubyValue::Nil);
        }
        let Some(needle) = arg else {
            return Err(crate::builtins::arity_err(0, 1, Some(1)));
        };
        for i in (0..len).rev() {
            let e = match arr.lock().get(i).cloned() {
                Some(e) => e,
                None => continue,
            };
            if e.rb_eq(needle) {
                return Ok(RubyValue::Int(i as i64));
            }
        }
        Ok(RubyValue::Nil)
    }
    // `rfind` is `find` scanning from the right -- the last element the block
    // accepts (nil if none); a blockless call answers an Enumerator.
    def "rfind" cfunc (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let items = rary.lock().clone();
        for e in items.iter().rev() {
            if p.call(std::slice::from_ref(e))?.truthy() {
                return Ok(e.clone());
            }
        }
        Ok(RubyValue::Nil)
    }
    def "dig" cfunc (recv, key, *rest, &_block) {
        let cur = index_only(recv, key)?;
        if rest.is_empty() {
            return Ok(cur);
        }
        // After our own first index, remaining keys recurse through the
        // intermediate's OWN `dig` (real Ruby's rule) -- a non-diggable there
        // raises TypeError rather than being indexed via some unrelated `[]`.
        crate::dispatch::obj_dig(cur, rest)
    }
    def "fetch" cfunc (recv, arg1, arg2?, &block) {
        if block.is_some() && arg2.is_some() {
            crate::builtins::warning::rb_warn("block supersedes default value argument");
        }
        let i = arg_int!(arg1);
        let items = rary.lock().clone();
        let n = items.len() as i64;
        let idx = if i < 0 { i + n } else { i };
        if (0..n).contains(&idx) {
            return Ok(items[idx as usize].clone());
        }
        // The BLOCK outranks a positional default -- which is what the warning
        // above says. It is yielded the ORIGINAL index, not the wrapped one.
        if let Some(RubyValue::Proc(p)) = &block {
            return p.call(std::slice::from_ref(arg1));
        }
        if let Some(default) = arg2 {
            return Ok(default.clone());
        }
        if let Some(RubyValue::Proc(p)) = &block {
            return p.call(&[RubyValue::Int(i)]);
        }
        Err(index_error!("index {i} outside of array bounds: {}...{n}", -n))
    }
    // `fetch_values(*indices)` -- each index fetched strictly (an out-of-range
    // index raises IndexError, or is passed to the block if one is given).
    def "fetch_values" params "*indexes, &block"(recv, *args, &block) {
        let items = rary.lock().clone();
        let n = items.len() as i64;
        let mut out = Vec::with_capacity(args.len());
        for arg in args {
            let i = convert::to_index(arg)?;
            let idx = if i < 0 { i + n } else { i };
            if (0..n).contains(&idx) {
                out.push(items[idx as usize].clone());
            } else if let Some(RubyValue::Proc(p)) = &block {
                out.push(p.call(&[RubyValue::Int(i)])?);
            } else {
                return Err(index_error!("index {i} outside of array bounds: {}...{n}", -n));
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "delete" (recv, arg, &block) {
        let handle = rary;
        check_frozen(handle, recv)?;
        // Two phases so a user `==` never runs under the lock: probe per
        // element, then retain by identity of the matched slots. The LAST
        // matched element is kept, because that -- not the argument -- is what
        // ruby answers: `[Always.new].delete(1)` gives back the `Always`.
        let mut matched = Vec::new();
        let mut last_match: Option<RubyValue> = None;
        let mut i = 0usize;
        loop {
            let e = {
                let guard = handle.lock();
                match guard.get(i) {
                    Some(e) => e.clone(),
                    None => break,
                }
            };
            if crate::builtins::basic_object::rb_equal(&e, arg)? {
                matched.push(i);
                last_match = Some(e);
            }
            i += 1;
        }
        if !matched.is_empty() {
            let mut guard = handle.lock();
            let mut idx = 0usize;
            guard.retain(|_| {
                let hit = matched.binary_search(&idx).is_ok();
                idx += 1;
                !hit
            });
            return Ok(last_match.unwrap_or_else(|| (*arg).clone()));
        }
        // Not found: a block supplies the answer (yielded the searched value),
        // else nil.
        match &block {
            Some(RubyValue::Proc(p)) => p.call(&[(*arg).clone()]),
            _ => Ok(RubyValue::Nil),
        }
    }
    def "delete_at" (recv, arg) {
        check_frozen(rary, recv)?;
        let i = arg_int!(arg);
        let handle = rary;
        let mut guard = handle.lock();
        let n = guard.len() as i64;
        let idx = if i < 0 { i + n } else { i };
        Ok(if (0..n).contains(&idx) {
            guard.remove(idx as usize)
        } else {
            RubyValue::Nil
        })
    }
    def "insert" cfunc (recv, _index, *_objects, &_block) {
        let args = __args;
        let handle = rary;
        // The frozen check comes FIRST, before the index is coerced --
        // `rb_ary_modify_check` sits at the top of every mutator, so a frozen
        // receiver is a FrozenError whatever the index turns out to be.
        check_frozen(handle, recv)?;
        let orig = arg_int!(args, 0);
        let mut guard = handle.lock();
        let n = guard.len() as i64;
        let at = if orig < 0 { orig + n + 1 } else { orig };
        if at < 0 {
            return Err(index_error!("index {orig} too small for array; minimum: {}", -n - 1));
        }
        let at = at as usize;
        // An index past the end pads the gap with nils (CRuby's rule), rather
        // than clamping the insertion to the current length.
        if at > guard.len() {
            guard.resize(at, RubyValue::Nil);
        }
        for (offset, v) in args[1..].iter().enumerate() {
            guard.insert(at + offset, v.clone());
        }
        drop(guard);
        Ok(recv.clone())
    }
    def "zip"(recv, *args, &block) {
        let base = rary.lock().clone();
        // CRuby's `take_items`: each source through `rb_check_array_type`
        // (`to_ary` ducks accepted); a non-convertible source falls back to
        // iterating its own `each` (a Range, an Enumerator), and only a
        // source with no `each` at all raises the respond-to shape.
        let others: Vec<Vec<RubyValue>> = args
            .iter()
            .map(|a| match convert::check_to_ary(a)? {
                Some(RubyValue::Array(x)) => Ok(x.lock().to_vec()),
                _ => take_items_via_each(a, base.len()),
            })
            .collect::<Result<_, _>>()?;
        let out = base
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut row = vec![e.clone()];
                for o in &others {
                    row.push(o.get(i).cloned().unwrap_or(RubyValue::Nil));
                }
                RubyValue::Array(crate::array_new(row))
            })
            .collect();
        // The BLOCK form yields each row and answers nil, rather than
        // building the result array at all -- real Ruby's own rule
        // (`[1,2].zip([3,4]) { |r| }` is nil, oracle-checked).
        if let Some(b) = block {
            for row in out {
                b.as_proc_unchecked().call(&[row])?;
            }
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "rotate"(recv, by?) {
        let by = match by {
            None => 1,
            Some(v) => arg_int!(v),
        };
        let mut out = rary.lock().to_vec();
        if !out.is_empty() {
            let n = out.len() as i64;
            let by = by.rem_euclid(n) as usize;
            out.rotate_left(by);
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `values_at(0, 2, -1)` / `values_at(0..1, 3..)` -- each argument is an
    // index OR a range of them, and an out-of-bounds index contributes nil
    // rather than being skipped.
    //
    // A range past the end still contributes one entry PER INDEX it names,
    // which is the non-obvious part: on a 5-element array,
    // `values_at(3..9)` is `[4, 5, nil, nil, nil, nil, nil]` (7 entries,
    // for indices 3..9) -- oracle-verified. So the range is expanded to its
    // indices and each looked up exactly as a bare Int index would be,
    // rather than being clamped to the array like `arr[3..9]` slicing is.
    def "values_at"(recv, *args, &_block) {
        let items = rary.lock().clone();
        let n = items.len() as i64;
        let at = |i: i64| {
            let idx = if i < 0 { i + n } else { i };
            if (0..n).contains(&idx) {
                items[idx as usize].clone()
            } else {
                RubyValue::Nil
            }
        };
        let mut out = Vec::new();
        for a in args {
            match a {
                RubyValue::Range(__rg) => {
                    let (start, end, exclusive) = __rg.parts();
                    let s = match start {
                        Some(v) => {
                            let v = convert::to_index(v)?;
                            if v < 0 { v + n } else { v }
                        }
                        None => 0,
                    };
                    // An endless range stops at the array's end -- it names
                    // no index past it, unlike a bounded one.
                    let e = match end {
                        Some(v) => {
                            let v = convert::to_index(v)?;
                            let v = if v < 0 { v + n } else { v };
                            if exclusive { v - 1 } else { v }
                        }
                        None => n - 1,
                    };
                    for i in s..=e {
                        out.push(at(i));
                    }
                }
                other => out.push(at(convert::to_index(other)?)),
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "at" (recv, arg) {
        let i = arg_int!(arg);
        Ok(crate::array_get(rary, i))
    }
    // `fill` writes a contiguous span, optionally growing the array.
    // Value form: `fill(obj[, start[, length]])`. Block form:
    // `fill([start[, length]]) { |i| }` -- the block maps each index to its
    // value. `start` is end-relative when negative; with no `length` the
    // span runs to the current end (so a `start` past the end is a no-op);
    // with `length`, `start + length` may extend past the end, back-filling
    // any gap with nil.
    def "fill"(recv, *args, &block) {
        let handle = rary;
        check_frozen(handle, recv)?;
        let cur_len = handle.lock().len() as i64;
        // With a block the whole list is the span; without one the first
        // argument is the fill VALUE and the rest is the span -- so the block
        // shifts the minimum by one, which no single parameter list can say.
        let (value, span): (Option<RubyValue>, &[RubyValue]) = match &block {
            Some(RubyValue::Proc(_)) => {
                crate::builtins::check_arity(args.len(), 0, Some(2))?;
                (None, args)
            }
            _ => {
                crate::builtins::check_arity(args.len(), 1, Some(3))?;
                (Some(args[0].clone()), &args[1..])
            }
        };
        // The position may be a Range (`fill(1..2) { }` / `fill(obj, 1..2)`)
        // or a `start[, length]` pair.
        let (start, end) = if let Some(RubyValue::Range(__rg)) = span.first() {
            let (rs, re, exclusive) = __rg.parts();
            let start = match rs {
                Some(v) => {
                    let v = convert::to_index(v)?;
                    if v < 0 { v + cur_len } else { v }
                }
                None => 0,
            };
            let end = match re {
                Some(v) => {
                    let v = convert::to_index(v)?;
                    let v = if v < 0 { v + cur_len } else { v };
                    if exclusive { v } else { v + 1 }
                }
                None => cur_len,
            };
            (start.max(0), end)
        } else {
            // A nil `start`/`length` is treated as absent (CRuby's rule:
            // `[1, 2].fill(0, nil)` fills from 0, no TypeError).
            let start = match span.first() {
                None | Some(RubyValue::Nil) => 0,
                Some(v) => {
                    let s = convert::to_index(v)?;
                    if s < 0 { s + cur_len } else { s }
                }
            };
            // A negative start beyond the array's length clamps to 0 -- fill
            // never raises here, unlike the `arr[i]=` assignment forms.
            let start = start.max(0);
            let end = match span.get(1) {
                None | Some(RubyValue::Nil) => cur_len,
                Some(v) => start + convert::to_index(v)?.max(0),
            };
            (start, end)
        };
        if end > cur_len {
            handle.lock().resize(end as usize, RubyValue::Nil);
        }
        for i in start..end {
            let v = match (&value, &block) {
                (Some(obj), _) => obj.clone(),
                // Compute the block's value WITHOUT holding the array lock --
                // the block may itself touch the array.
                (None, Some(RubyValue::Proc(p))) => p.call(&[RubyValue::Int(i)])?,
                _ => unreachable!("fill has either a value or a block"),
            };
            handle.lock()[i as usize] = v;
        }
        Ok(recv.clone())
    }
    def "clear" (recv) {
        check_frozen(rary, recv)?;
        rary.lock().clear();
        Ok(recv.clone())
    }
    def "replace" (recv, arg) {
        check_frozen(rary, recv)?;
        let other = &convert::to_rary(arg)?;
        let new_items = other.lock().clone();
        *rary.lock() = new_items;
        Ok(recv.clone())
    }
    // ruby's `rb_ary_initialize`, as the real private row: `Array.new`'s
    // whole surface applied IN PLACE, so `[].send(:initialize, 2, 0)` is
    // `[0, 0]` and a subclass `super` re-seeds the same storage.
    private def "initialize"(recv, size?, fill?, &block) {
        check_frozen(rary, recv)?;
        if fill.is_none()
            && let Some(RubyValue::Array(a)) = size
        {
            let items = a.lock().to_vec();
            *rary.lock() = items.into();
            return Ok(recv.clone());
        }
        let size = match size {
            None => 0,
            Some(v) => arg_int!(v),
        };
        if size < 0 {
            return Err(arg_error!("negative array size"));
        }
        let size = size as usize;
        let items: Vec<RubyValue> = if let Some(RubyValue::Proc(p)) = &block {
            let mut out = Vec::with_capacity(size);
            for i in 0..size {
                out.push(p.call(&[RubyValue::Int(i as i64)])?);
            }
            out
        } else {
            vec![fill.cloned().unwrap_or(RubyValue::Nil); size]
        };
        *rary.lock() = items.into();
        Ok(recv.clone())
    }
    // `initialize_copy` IS `replace` in CRuby (`rb_ary_replace` backs both).
    private def "initialize_copy"(recv, other) {
        check_frozen(rary, recv)?;
        let other = &convert::to_rary(other)?;
        let new_items = other.lock().clone();
        *rary.lock() = new_items;
        Ok(recv.clone())
    }
    // `sort` with rb_cmp or a comparator block; `sort!` in place.
    def "sort" (recv, &block) {
        let mut items = rary.lock().to_vec();
        sort_items(&mut items, &block)?;
        Ok(RubyValue::Array(crate::array_new(items)))
    }
    def "sort!" (recv, &block) {
        check_frozen(rary, recv)?;
        let mut items = rary.lock().to_vec();
        sort_items(&mut items, &block)?;
        *rary.lock() = items.into();
        Ok(recv.clone())
    }
    def "map!" | "collect!" (recv, &block) {
        check_frozen(rary, recv)?;
        let p = block_or_enum!(recv, &[], block);
        let items = rary.lock().to_vec();
        let mut out = Vec::with_capacity(items.len());
        for e in items {
            out.push(p.call(&[e])?);
        }
        *rary.lock() = out.into();
        Ok(recv.clone())
    }
    // In-place filters: self when anything changed, nil otherwise (real
    // Ruby's contract).
    def "select!" | "filter!" (recv, &block) {
        in_place_filter(recv, __RUBY_METHOD, block, true)
    }
    def "reject!" (recv, &block) {
        in_place_filter(recv, "reject!", block, false)
    }
    // keep_if/delete_if return SELF (not self-or-nil), so their blockless
    // Enumerator must short-circuit before the self return.
    def "keep_if" (recv, &block) {
        if !matches!(block, Some(RubyValue::Proc(_))) {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "keep_if", &[]));
        }
        in_place_filter(recv, "keep_if", block, true)?;
        Ok(recv.clone())
    }
    def "delete_if" (recv, &block) {
        if !matches!(block, Some(RubyValue::Proc(_))) {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "delete_if", &[]));
        }
        in_place_filter(recv, "delete_if", block, false)?;
        Ok(recv.clone())
    }
    // `sample` answers ONE random element (nil when empty); `sample(n)` an
    // ARRAY of up to n DISTINCT elements (a partial Fisher-Yates shuffle).
    // Shares Kernel#rand's generator, which `srand` reseeds.
    // A `random:` keyword supplies the RNG (a Random-like object responding to
    // `rand`); without it the shared PRNG is used.
    ruby def "sample"(recv, n?, random:?) {
        let items = rary.lock().clone();
        let len = items.len();
        let Some(v) = n else {
            return Ok(if len == 0 {
                RubyValue::Nil
            } else {
                items[rand_below(&random, len)?].clone()
            });
        };
        // `sample`'s own negative message, checked AFTER conversion (so
        // `sample(-2.9)` truncates first, then complains) -- oracle shape.
        let n = convert::to_index(v)?;
        if n < 0 {
            return Err(arg_error!("negative sample number"));
        }
        // Ruby draws once per pick, over a range that narrows by one each
        // time, and has two ways of turning that draw into a distinct index.
        // Up to ten picks it keeps them in a small sorted list and shifts the
        // draw past every index already taken; past that it runs a VIRTUAL
        // partial Fisher-Yates, reading each position through a map of where
        // an earlier swap moved it. Both are reproduced here because a
        // seeded `sample` is expected to replay ruby's own answer.
        const SORTED_PICKS_MAX: usize = 10;
        let take = (n as usize).min(len);
        let mut out = Vec::with_capacity(take);
        if take <= SORTED_PICKS_MAX {
            let mut taken: Vec<usize> = Vec::with_capacity(take);
            for i in 0..take {
                let mut j = rand_below(&random, len - i)?;
                let mut at = taken.len();
                for (pos, &t) in taken.iter().enumerate() {
                    if j < t {
                        at = pos;
                        break;
                    }
                    j += 1;
                }
                taken.insert(at, j);
                out.push(items[j].clone());
            }
        } else {
            let mut moved: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
            for i in 0..take {
                let j = i + rand_below(&random, len - i)?;
                let from = *moved.get(&i).unwrap_or(&i);
                let read = *moved.get(&j).unwrap_or(&j);
                moved.insert(j, from);
                out.push(items[read].clone());
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    ruby def "shuffle"(recv, random:?) {
        let mut items = rary.lock().to_vec();
        // Fisher-Yates, top down, exactly as ruby walks it.
        for i in (1..items.len()).rev() {
            let j = rand_below(&random, i + 1)?;
            items.swap(i, j);
        }
        Ok(RubyValue::Array(crate::array_new(items)))
    }
    // `pack`: serialize the elements per a template into a byte string (see
    // `builtins::pack`). ASCII-8BIT unless the template is all `U` (UTF-8).
    def "pack" arity -2 params "fmt, buffer: nil"(recv, arg) {
        let t = crate::builtins::arg_str!(arg);
        let template = t.lock().to_utf8_lossy().into_owned();
        let elems = rary.lock().clone();
        let bytes = crate::builtins::pack::pack(&elems, &template)?;
        Ok(RubyValue::Str(crate::string_from_bytes(
            bytes,
            crate::builtins::pack::result_encoding(&template),
        )))
    }
    // With a block, each element is MAPPED to its pair first -- an Array
    // yields one value per element, so the block sees the element itself
    // (`[[1, 2]].to_h { |pair| }` gets `[1, 2]`; `{ |a, b| }` auto-splats).
    def "to_h" (recv, &block) {
        let items = rary.lock().clone();
        let pairs = crate::builtins::enumerable::to_h_pairs(
            items.iter().map(|e| (std::slice::from_ref(e), e)),
            &block,
        )?;
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    def "each" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        // Live view, one lock round-trip per element: appending from inside
        // the block iterates the appended tail and shrinking stops early --
        // CRuby's own rule -- and the old whole-Vec snapshot per call (which
        // made every Enumerable method driving `each` quadratic in a loop)
        // is gone. The lock is never held across the block call.
        let arr = rary;
        let mut i = 0usize;
        loop {
            let e = {
                let guard = arr.lock();
                match guard.get(i) {
                    Some(e) => e.clone(),
                    None => break,
                }
            };
            p.call(&[e])?;
            i += 1;
        }
        Ok(recv.clone())
    }
    def "each_index" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let n = rary.lock().len();
        for i in 0..n {
            p.call(&[RubyValue::Int(i as i64)])?;
        }
        Ok(recv.clone())
    }
    // `combination(n)`/`permutation(n)`: the block form yields each tuple
    // and answers the receiver; the blockless form answers an Enumerator
    // (`block_or_enum!`), which is what makes the idiomatic
    // `.combination(2).to_a` work.
    //
    // Ruby's own edge cases, oracle-verified, and the reason the bounds
    // aren't just `n > len => empty`:
    //   [1,2,3].combination(0) => [[]]   -- ONE empty tuple, not none
    //   [1,2,3].combination(4) => []
    //   [1,2].permutation      => the FULL-length permutations (no arg)
    def "combination"(recv, n, &block) {
        let n = arg_int!(n);
        let p = block_or_enum!(recv, __args, block);
        let items = rary.lock().clone();
        for tuple in combinations_of(&items, n) {
            p.call(&[RubyValue::Array(crate::array_new(tuple))])?;
        }
        Ok(recv.clone())
    }
    def "permutation"(recv, n?, &block) {
        let items = rary.lock().clone();
        // No argument means the receiver's own length -- read BEFORE
        // `block_or_enum!`'s early return so the Enumerator it builds
        // re-invokes with the identical (empty) argument list.
        let n = match n {
            Some(v) => arg_int!(v),
            None => items.len() as i64,
        };
        let p = block_or_enum!(recv, __args, block);
        for tuple in permutations_of(&items, n) {
            p.call(&[RubyValue::Array(crate::array_new(tuple))])?;
        }
        Ok(recv.clone())
    }
    // The with-repetition siblings: `repeated_permutation` is `items`^n
    // (order matters, repeats allowed); `repeated_combination` is the
    // non-decreasing multisets. Both take a required length and yield tuples
    // (or return an Enumerator without a block).
    def "repeated_permutation"(recv, n, &block) {
        // The Enumerator comes FIRST, exactly as `RETURN_SIZED_ENUMERATOR`
        // does, so a blockless call DEFERS the argument check to iteration --
        // `[1].repeated_permutation("l")` answers an Enumerator that prints
        // the bad argument, and only raises when driven.
        let p = block_or_enum!(recv, __args, block);
        let n = arg_int!(n);
        let items = rary.lock().clone();
        for tuple in repeated_permutations_of(&items, n) {
            p.call(&[RubyValue::Array(crate::array_new(tuple))])?;
        }
        Ok(recv.clone())
    }
    def "repeated_combination"(recv, n, &block) {
        let n = arg_int!(n);
        let p = block_or_enum!(recv, __args, block);
        let items = rary.lock().clone();
        for tuple in repeated_combinations_of(&items, n) {
            p.call(&[RubyValue::Array(crate::array_new(tuple))])?;
        }
        Ok(recv.clone())
    }
    // `intersect?(other)` -- do the two arrays share any element? (uses the
    // same `rb_eq` membership as `&`/`intersection`, no result array built).
    def "intersect?" (recv, arg) {
        let other = &convert::to_rary(arg)?;
        let mine = rary.lock().clone();
        let theirs = other.lock().clone();
        Ok(RubyValue::Bool(
            mine.iter().any(|e| theirs.iter().any(|x| e.rb_eq(x))),
        ))
    }
    // No `chain` row: ruby owns it on Enumerable alone, and the body here was
    // byte-identical to Enumerable's, so the ancestry reaches the same code.
    // `compact!` drops nils in place, answering `nil` when there were none
    // (CRuby's destructive-form convention); `rotate!` rotates in place and
    // always answers the receiver.
    def "compact!" (recv) {
        let handle = rary;
        check_frozen(handle, recv)?;
        let before = handle.lock().len();
        let kept: Vec<RubyValue> = handle
            .lock()
            .iter()
            .filter(|e| !matches!(e, RubyValue::Nil))
            .cloned()
            .collect();
        if kept.len() == before {
            return Ok(RubyValue::Nil);
        }
        *handle.lock() = kept.into();
        Ok(recv.clone())
    }
    def "rotate!"(recv, count?) {
        check_frozen(rary, recv)?;
        let n = match count {
            None => 1,
            Some(v) => arg_int!(v),
        };
        let handle = rary;
        let mut items = handle.lock().clone();
        let len = items.len();
        if len > 0 {
            let shift = n.rem_euclid(len as i64) as usize;
            items.rotate_left(shift);
        }
        *handle.lock() = items;
        Ok(recv.clone())
    }
    // Pattern-matching / implicit-conversion hooks: an Array deconstructs to
    // and converts as itself.
    def "deconstruct" | "to_ary" (recv) {
        Ok(recv.clone())
    }
    // In-place Fisher-Yates shuffle (mirrors `shuffle` but writes back and
    // answers the receiver).
    ruby def "shuffle!"(recv, random:?) {
        let handle = rary;
        check_frozen(handle, recv)?;
        let mut items = handle.lock().clone();
        for i in (1..items.len()).rev() {
            let j = rand_below(&random, i + 1)?;
            items.swap(i, j);
        }
        *handle.lock() = items;
        Ok(recv.clone())
    }
    // `cycle(n)` repeats the whole array n times; `cycle` with no argument
    // repeats FOREVER (so it only terminates via `break`) -- that infinite
    // form is why this can't just materialize the repeated array.
    // `cycle(0)`/`cycle(-1)` yield nothing at all.
    def "cycle"(recv, count?, &block) {
        let count = match count {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(arg_int!(v)),
        };
        let p = block_or_enum!(recv, __args, block);
        let items = rary.lock().clone();
        // An empty receiver never yields, and would spin forever below.
        if items.is_empty() {
            return Ok(RubyValue::Nil);
        }
        let mut remaining = count.unwrap_or(1);
        while remaining > 0 {
            for e in &items {
                p.call(std::slice::from_ref(e))?;
            }
            if count.is_some() {
                remaining -= 1;
            }
        }
        Ok(RubyValue::Nil)
    }
    // `bsearch`/`bsearch_index` in FIND-MINIMUM mode: the block answers
    // true/false and the array must be sorted so that every false precedes
    // every true; the answer is the first true (`nil` if there is none).
    // CRuby also has a find-any mode (the block answering an Integer);
    // that's a documented gap -- a numeric block result raises rather than
    // silently treating it as truthy and answering the wrong element.
    // Blockless answers an Enumerator, as every `RETURN_ENUMERATOR` row does --
    // it was raising `no block given (yield)` instead.
    def "bsearch" (recv, &block) {
        let p = block_or_enum!(recv, __args, block);
        let items = rary.lock().clone();
        Ok(match bsearch_find(&items, Some(RubyValue::Proc(p)))? {
            Some(i) => items[i].clone(),
            None => RubyValue::Nil,
        })
    }
    def "bsearch_index" (recv, &block) {
        let p = block_or_enum!(recv, __args, block);
        let items = rary.lock().clone();
        Ok(match bsearch_find(&items, Some(RubyValue::Proc(p)))? {
            Some(i) => RubyValue::Int(i as i64),
            None => RubyValue::Nil,
        })
    }
    // `flatten!`/`sort_by!`: the bang forms mutate in place. `flatten!`
    // answers nil when NOTHING changed (Ruby's convention for the
    // destructive forms); `sort_by!` always answers the receiver.
    def "flatten!"(recv, depth?) {
        let depth = match depth {
            None | Some(RubyValue::Nil) => -1,
            Some(v) => arg_int!(v),
        };
        let cell = rary;
        check_frozen(cell, recv)?;
        let before = cell.lock().clone();
        let after = flatten_to_depth(&before, depth)?;
        if after.len() == before.len()
            && after.iter().zip(before.iter()).all(|(a, b)| a.rb_eq(b))
        {
            return Ok(RubyValue::Nil);
        }
        *cell.lock() = after.into();
        Ok(recv.clone())
    }
    def "sort_by!" (recv, &block) {
        check_frozen(rary, recv)?;
        let p = block_or_enum!(recv, &[], block);
        let cell = rary;
        let items = cell.lock().clone();
        // Decorate-sort-undecorate: the block runs once per element, as
        // CRuby's does, rather than once per comparison.
        let mut keyed: Vec<(RubyValue, RubyValue)> = Vec::with_capacity(items.len());
        for e in items {
            keyed.push((p.call(std::slice::from_ref(&e))?, e));
        }
        keyed.sort_by(|a, b| match a.0.rb_cmp(&b.0) {
            Some(o) => o.cmp(&0),
            None => std::cmp::Ordering::Equal,
        });
        *cell.lock() = keyed.into_iter().map(|(_, e)| e).collect();
        Ok(recv.clone())
    }

    // ---- rows ruby OWNS on Array while the body lives on an ancestor.
    //
    // Declaring them here changes `.owner` and `instance_methods(false)` and
    // NOTHING else: each one calls the very row it would otherwise have
    // inherited, so the two can never drift apart. The parameter lists are
    // the oracle's, which is why some take a splat where the ancestor's
    // signature is narrower.
    def "map" arity 0 | "collect" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::map_own(s, __args, block, __RUBY_METHOD)) }
    def "select" arity 0 | "filter" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::select(s, __args, block, true, __RUBY_METHOD)) }
    def "reject" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::select(s, __args, block, false, __RUBY_METHOD)) }
    def "find" | "detect" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::find_own(s, __args, block, __RUBY_METHOD)) }
    def "all?" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::any_all(s, __args, block, enumerable::Quantifier::All)) }
    def "any?" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::any_all(s, __args, block, enumerable::Quantifier::Any)) }
    def "none?" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::any_all(s, __args, block, enumerable::Quantifier::None)) }
    def "one?" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::any_all(s, __args, block, enumerable::Quantifier::One)) }
    def "count" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::count_own(s, __args, block)) }
    def "sum" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::sum_own(s, __args, block)) }
    def "max" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::min_max(s, __args, block, false)) }
    def "min" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::min_max(s, __args, block, true)) }
    def "minmax" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::minmax_own(s, __args, block)) }
    def "take" arity 1 (recv, *_args, &_block) { own_row!(recv, |s| enumerable::take_drop(s, __args, true)) }
    def "drop" arity 1 (recv, *_args, &_block) { own_row!(recv, |s| enumerable::take_drop(s, __args, false)) }
    def "take_while" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::take_while_own(s, __args, block)) }
    def "drop_while" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::drop_while_own(s, __args, block)) }
    def "reverse_each" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::reverse_each_own(s, __args, block)) }
    def "freeze"(recv) { inherited_row!(kernel, "freeze", recv, __args, None) }
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
    def "inspect"(recv) { inherited_row!(kernel, "inspect", recv, __args, None) }
    // NOT an alias of `#inspect`: `Complex`, `Rational` and `Regexp` all
    // spell the two differently, so each goes to its own Kernel row.
    def "to_s"(recv) { inherited_row!(kernel, "to_s", recv, __args, None) }
}

/// Coerces every argument of a variadic set op (`union`/`intersection`/
/// `difference`) to its element vector through the `to_ary` protocol.
fn set_op_args(args: &[RubyValue]) -> Result<Vec<Vec<RubyValue>>, crate::Signal> {
    args.iter()
        .map(|a| Ok(convert::to_rary(a)?.lock().to_vec()))
        .collect()
}

/// The uniq'd concatenation `self ++ others...`, first occurrence winning --
/// shared by `|` (binary) and `union` (variadic).
fn union_of(recv: &crate::collections::RArray, others: &[Vec<RubyValue>]) -> Vec<RubyValue> {
    let mut out: Vec<RubyValue> = Vec::new();
    for e in recv.lock().iter().chain(others.iter().flatten()) {
        if !out.iter().any(|x| e.rb_eq(x)) {
            out.push(e.clone());
        }
    }
    out
}

/// A random index in `0..bound`, from the supplied RNG (`random.rand(bound)`)
/// or the shared PRNG. `bound` is assumed nonzero by the callers.
fn rand_below(random: &Option<RubyValue>, bound: usize) -> Result<usize, crate::Signal> {
    match random {
        Some(rng) => {
            let r = crate::dispatch::send_value(
                rng,
                crate::Symbol::intern("rand"),
                &[RubyValue::Int(bound as i64)],
                None,
            )?;
            Ok((convert::to_index(&r)?.rem_euclid(bound as i64)) as usize)
        }
        None => Ok(crate::builtins::kernel::prng_limited(bound as u64 - 1) as usize),
    }
}

/// Raise `FrozenError` if `recv` (an Array) is frozen -- the guard every
/// mutating method runs before touching its storage.
#[inline]
fn check_frozen(
    handle: &crate::collections::RArray,
    recv: &RubyValue,
) -> Result<(), crate::Signal> {
    if handle.is_frozen() {
        return Err(frozen_array_error(recv));
    }
    Ok(())
}

#[cold]
fn frozen_array_error(recv: &RubyValue) -> crate::Signal {
    crate::dispatch::raise_error_details(
        "FrozenError",
        format!("can't modify frozen Array: {}", recv.inspect_string()),
        &[("receiver", recv.clone())],
    )
}

/// The typed-receiver fast-path cores for the bare mutators
/// (`codegen`'s `try_collection_dispatch`): the same
/// frozen-check-then-mutate the builtin defs run, minus the dispatch and
/// argument plumbing. Each mirrors its def's no-count branch exactly.
pub fn array_push_checked(
    arr: &crate::collections::RArray,
    value: RubyValue,
) -> Result<RubyValue, crate::Signal> {
    check_frozen_handle(arr)?;
    Ok(crate::array_push(arr, value))
}

pub fn array_pop_checked(arr: &crate::collections::RArray) -> Result<RubyValue, crate::Signal> {
    check_frozen_handle(arr)?;
    Ok(arr.lock().pop().unwrap_or(RubyValue::Nil))
}

pub fn array_shift_checked(arr: &crate::collections::RArray) -> Result<RubyValue, crate::Signal> {
    check_frozen_handle(arr)?;
    Ok(arr.lock().shift().unwrap_or(RubyValue::Nil))
}

/// [`check_frozen`] for callers that hold only the handle: the receiver
/// `RubyValue` (an `Arc` clone) is materialized in the COLD arm only --
/// building it eagerly put an atomic refcount round-trip on every push/pop/
/// shift fast path, which is exactly the traffic `bm_so_lists` drowns in.
#[inline]
fn check_frozen_handle(arr: &crate::collections::RArray) -> Result<(), crate::Signal> {
    if arr.is_frozen() {
        return Err(frozen_array_error(&RubyValue::Array(arr.clone())));
    }
    Ok(())
}

/// `Array#join`'s body: appends each element's bytes to `out`, separated by
/// `sep`, and flattens nested arrays recursively under that same separator --
/// `[1, [2, 3]].join("-")` gives `"1-2-3"`.
///
/// Byte-faithful: a String element contributes its own bytes under its own
/// encoding, and only a non-String goes through `to_s`.
fn join_into(
    out: &mut crate::enc::StrBuf,
    elems: &[RubyValue],
    sep: Option<&crate::enc::StrBuf>,
) -> Result<(), crate::Signal> {
    for (i, e) in elems.iter().enumerate() {
        if i > 0
            && let Some(sep) = sep
        {
            push_or_raise(out, sep)?;
        }
        match e {
            RubyValue::Array(inner) => join_into(out, &inner.lock().clone(), sep)?,
            RubyValue::Str(s) => push_or_raise(out, &s.lock().clone())?,
            other => {
                let text = crate::string_new(other.to_display_string());
                let text = text.lock().clone();
                push_or_raise(out, &text)?;
            }
        }
    }
    Ok(())
}

fn push_or_raise(
    out: &mut crate::enc::StrBuf,
    part: &crate::enc::StrBuf,
) -> Result<(), crate::Signal> {
    out.push_buf(part).map_err(|_| {
        crate::dispatch::raise_error(
            "Encoding::CompatibilityError",
            format!(
                "incompatible character encodings: {} and {}",
                out.encoding().inspect_name(),
                part.encoding().inspect_name()
            ),
        )
    })
}

/// Per-element `eql?` (class-strict): `1.eql?(1.0)` is false because Integer
/// and Float differ, whereas `==` coerces. Nested arrays compare element-wise
/// (identity short-circuits a self-reference); other values require the same
/// value kind plus `==`.
/// CRuby's `eql?` (the `uniq`/`Array#eql?` predicate), NOT `==`. It projects
/// through the same key `Hash` uses (`hash_key`), so `1.eql?(1.0)` is false
/// and -- crucially -- a user object that defines only `==` (no `eql?`/`hash`)
/// dedups by IDENTITY, so `[Point.new(1), Point.new(1)].uniq` keeps both. Using
/// `rb_eq` here would dispatch the user `==` and wrongly merge them.
fn values_eql(a: &RubyValue, b: &RubyValue) -> bool {
    crate::collections::hash_key(a) == crate::collections::hash_key(b)
}

/// Shared `uniq`/`uniq!` dedup, keeping the FIRST element of each group. With
/// no block, groups by the element's own `eql?`/`hash` (so `[1.0, 1]` keeps
/// both). With a block, groups by the block's return value instead, comparing
/// those keys by `eql?`/`hash` -- CRuby's `rb_ary_uniq` semantics.
fn uniq_dedup(
    items: &[RubyValue],
    block: Option<&RubyValue>,
) -> Result<Vec<RubyValue>, crate::Signal> {
    let mut out: Vec<RubyValue> = Vec::new();
    match block {
        None => {
            for e in items {
                if !out.iter().any(|x| values_eql(e, x)) {
                    out.push(e.clone());
                }
            }
        }
        Some(b) => {
            let p = b.as_proc_unchecked();
            let mut keys: Vec<RubyValue> = Vec::new();
            for e in items {
                let key = p.call(std::slice::from_ref(e))?;
                if !keys.iter().any(|k| values_eql(&key, k)) {
                    keys.push(key);
                    out.push(e.clone());
                }
            }
        }
    }
    Ok(out)
}

/// The optional COUNT argument of `pop(n)`/`shift(n)`/`last(n)`: `None` when
/// absent (the answer-one-element form), else the count through the
/// `to_int` protocol. A negative one is CRuby's "negative array size"
/// ArgumentError (checked after conversion, so `pop(-2.9)` truncates first).
fn count_arg(v: Option<&RubyValue>) -> Result<Option<usize>, crate::Signal> {
    let Some(v) = v else {
        return Ok(None);
    };
    let n = convert::to_index(v)?;
    if n < 0 {
        return Err(arg_error!("negative array size"));
    }
    Ok(Some(n as usize))
}

/// The INDEX of the first element the block answers truthy for, in
/// FIND-MINIMUM mode: `items` must be sorted so every false precedes every
/// true, and the search is the ordinary binary one over that boundary.
/// Shared by `bsearch` (which wants the element) and `bsearch_index`.
///
/// CRuby also has a find-any mode, selected by the block answering an
/// Integer instead of a boolean. That's a documented gap -- and it RAISES
/// rather than falling through, because a numeric result is truthy, so
/// treating it as the boolean mode would silently answer the wrong element
/// instead of failing.
/// The shared binary search behind `bsearch`/`bsearch_index` for a sorted
/// slice, covering both CRuby modes selected by the block's return type:
///
/// * **find-minimum** (block answers a boolean/nil): the slice is partitioned
///   false-then-true; the answer is the index of the first true (`None` if
///   none), the classic lower-bound.
/// * **find-any** (block answers a Numeric, the comparator protocol): `0` is a
///   hit at that index, a negative result searches the lower half, a positive
///   result the upper half; `None` when no element answers `0`.
///
/// A single call stays in one mode (CRuby raises on a mix; the corpus never
/// does, so this simply follows whichever branch each result takes).
fn bsearch_find(
    items: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<Option<usize>, crate::Signal> {
    let p = crate::builtins::need_block!(block);
    let (mut lo, mut hi) = (0usize, items.len());
    let mut numeric_mode = false;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let r = p.call(&[items[mid].clone()])?;
        let cmp = match r {
            RubyValue::Int(n) => Some(n.cmp(&0)),
            RubyValue::Float(f) => f.partial_cmp(&0.0),
            _ => None,
        };
        match cmp {
            Some(std::cmp::Ordering::Equal) => return Ok(Some(mid)),
            Some(std::cmp::Ordering::Less) => {
                numeric_mode = true;
                hi = mid;
            }
            Some(std::cmp::Ordering::Greater) => {
                numeric_mode = true;
                lo = mid + 1;
            }
            // Boolean/nil find-minimum mode (a NaN Float also lands here and,
            // like CRuby, never matches -- it drives the search upward).
            None if r.truthy() => hi = mid,
            None => lo = mid + 1,
        }
    }
    // find-any exhausted the range without a `0` -> nil; find-minimum answers
    // the first true (the final `lo`), if any.
    Ok(if numeric_mode {
        None
    } else {
        (lo < items.len()).then_some(lo)
    })
}

/// Every `n`-element combination of `items`, in Ruby's order (indices
/// ascending, lexicographic). `n == 0` is ONE empty tuple; `n` past the
/// length, or negative, is none at all.
fn combinations_of(items: &[RubyValue], n: i64) -> Vec<Vec<RubyValue>> {
    if n < 0 || n as usize > items.len() {
        return Vec::new();
    }
    let n = n as usize;
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    let mut idx: Vec<usize> = (0..n).collect();
    loop {
        out.push(idx.iter().map(|&i| items[i].clone()).collect());
        // Advance the rightmost index that still has room, then repack the
        // ones after it -- the standard odometer over ascending indices.
        let Some(i) = (0..n).rev().find(|&i| idx[i] != i + items.len() - n) else {
            return out;
        };
        idx[i] += 1;
        for j in i + 1..n {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

/// Every length-`n` sequence drawn from `items` WITH repetition, in Ruby's
/// order (`items` cycled fastest in the last position) -- `items`^n. `n <= 0`
/// yields the single empty tuple only when `n == 0`.
fn repeated_permutations_of(items: &[RubyValue], n: i64) -> Vec<Vec<RubyValue>> {
    if n < 0 {
        return Vec::new();
    }
    let n = n as usize;
    let mut out = vec![Vec::new()];
    for _ in 0..n {
        let mut next = Vec::with_capacity(out.len() * items.len());
        for prefix in &out {
            for e in items {
                let mut t = prefix.clone();
                t.push(e.clone());
                next.push(t);
            }
        }
        out = next;
    }
    out
}

/// Every length-`n` multiset drawn from `items` (non-decreasing index
/// sequences) -- `repeated_combination`'s order.
fn repeated_combinations_of(items: &[RubyValue], n: i64) -> Vec<Vec<RubyValue>> {
    if n < 0 || (items.is_empty() && n > 0) {
        return Vec::new();
    }
    let n = n as usize;
    let mut out = Vec::new();
    let mut idx = vec![0usize; n];
    loop {
        out.push(idx.iter().map(|&i| items[i].clone()).collect());
        // Odometer over non-decreasing indices: bump the rightmost that can
        // still grow, then flatten the tail up to its value.
        let Some(i) = (0..n).rev().find(|&i| idx[i] + 1 < items.len()) else {
            return out;
        };
        let v = idx[i] + 1;
        for slot in idx.iter_mut().skip(i) {
            *slot = v;
        }
    }
}

/// Every `n`-element permutation of `items`, in Ruby's order. Same bounds
/// rules as `combinations_of`.
fn permutations_of(items: &[RubyValue], n: i64) -> Vec<Vec<RubyValue>> {
    if n < 0 || n as usize > items.len() {
        return Vec::new();
    }
    let n = n as usize;
    let mut out = Vec::new();
    let mut chosen: Vec<usize> = Vec::with_capacity(n);
    let mut used = vec![false; items.len()];
    walk_permutations(items, n, &mut chosen, &mut used, &mut out);
    out
}

fn walk_permutations(
    items: &[RubyValue],
    n: usize,
    chosen: &mut Vec<usize>,
    used: &mut Vec<bool>,
    out: &mut Vec<Vec<RubyValue>>,
) {
    if chosen.len() == n {
        out.push(chosen.iter().map(|&i| items[i].clone()).collect());
        return;
    }
    for i in 0..items.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        chosen.push(i);
        walk_permutations(items, n, chosen, used, out);
        chosen.pop();
        used[i] = false;
    }
}

/// `zip`'s `:each` fallback (CRuby `take_items`): a source that isn't
/// `to_ary`-convertible (a Range, an Enumerator) is iterated through its own
/// `each`, collecting up to `n` values -- the collector breaks out at `n`
/// exactly like CRuby's `rb_iter_break`, so an endless source terminates. A
/// source with no `each` at all keeps the respond-to TypeError.
fn take_items_via_each(src: &RubyValue, n: usize) -> Result<Vec<RubyValue>, crate::Signal> {
    let each = crate::symbol::wk::each();
    if !crate::dispatch::responds_to_value(src, each, false) {
        // NOT a `Check_Type` message: `rb_zip`'s own uses `rb_obj_class`,
        // so `nil` reads as `NilClass` here where the expected-Y form reads
        // it as the value. Oracle-verified.
        return Err(type_error!(
            "wrong argument type {} (must respond to :each)",
            crate::builtins::class_name_of(src)
        ));
    }
    let out = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = out.clone();
    let collector = crate::RProc::new(move |a: &[RubyValue]| {
        let mut g = sink.lock();
        // A multi-value yield arrives as one row, like a block's array param.
        g.push(match a {
            [one] => one.clone(),
            many => RubyValue::Array(crate::array_new(many.to_vec())),
        });
        if g.len() >= n {
            return Err(crate::Signal::Break(RubyValue::Nil));
        }
        Ok(RubyValue::Nil)
    });
    match crate::dispatch::send_value(src, each, &[], Some(RubyValue::Proc(collector))) {
        // The break either surfaces here or was absorbed by the iterator's
        // own block-attach site -- both mean "stopped at n", not an error.
        Ok(_) | Err(crate::Signal::Break(_)) => {}
        Err(e) => return Err(e),
    }
    let items = out.lock().clone();
    Ok(items)
}

/// Flattens nested arrays up to `depth` levels (`-1` = fully). Shared by
/// `flatten`, `flatten!` and `Hash#flatten`.
pub(crate) fn flatten_to_depth(
    items: &[RubyValue],
    depth: i64,
) -> Result<Vec<RubyValue>, crate::Signal> {
    flatten_into(items, depth, &mut Vec::new())
}

/// `seen` is the traversal STACK of arrays currently being flattened -- pushed
/// on descent and popped on ascent, so a genuine cycle raises while a merely
/// SHARED sub-array (`[b, b]`) still flattens twice. That is CRuby's rule and
/// its bookkeeping (`rb_ary_flatten`'s ident-hash memo, `rb_hash_delete` on
/// the way back up), and like CRuby the guard runs only at unlimited depth:
/// `a.flatten(1)` stops before re-entry and must not raise.
fn flatten_into(
    items: &[RubyValue],
    depth: i64,
    seen: &mut Vec<usize>,
) -> Result<Vec<RubyValue>, crate::Signal> {
    let mut out = Vec::new();
    for e in items {
        match e {
            RubyValue::Array(inner) if depth != 0 => {
                let guarded = depth < 0;
                let id = std::sync::Arc::as_ptr(inner) as usize;
                if guarded {
                    if seen.contains(&id) {
                        return Err(arg_error!("tried to flatten recursive array"));
                    }
                    seen.push(id);
                }
                let nested = inner.lock().clone();
                out.extend(flatten_into(&nested, depth - 1, seen)?);
                if guarded {
                    seen.pop();
                }
            }
            // Ruby flattens anything that CONVERTS to an Array, not only a
            // real one: `rb_ary_flatten` probes each element with
            // `rb_check_array_type`, so an object answering `to_ary` is
            // descended into. A probe that answers nil (the common case) is
            // just a non-array element.
            other if depth != 0 => match crate::builtins::convert::check_to_ary(other)? {
                Some(RubyValue::Array(inner)) => {
                    let nested = inner.lock().clone();
                    out.extend(flatten_into(&nested, depth - 1, seen)?);
                }
                _ => out.push(other.clone()),
            },
            other => out.push(other.clone()),
        }
    }
    Ok(out)
}

/// `arr[start, len] = value` / `arr[range] = value` -- CRuby's
/// `rb_ary_splice`: replaces the `start..start+len` span with `value`'s
/// `to_ary` coercion (see `splice_elems`), padding with `nil` when `start`
/// lies past the end.
fn array_splice(
    arr: &crate::RArray,
    start: i64,
    len: i64,
    value: &RubyValue,
) -> Result<(), crate::Signal> {
    if len < 0 {
        return Err(index_error!("negative length ({len})"));
    }
    let elems = splice_elems(value)?;
    let mut items = arr.lock();
    let n = items.len() as i64;
    let start = if start < 0 { start + n } else { start };
    if start < 0 {
        return Err(index_error!(
            "index {} too small for array; minimum: {}",
            start - n,
            -n
        ));
    }
    let start = start as usize;
    if start > items.len() {
        items.resize(start, RubyValue::Nil);
    }
    let end = (start + len as usize).min(items.len());
    items.vec().splice(start..end, elems);
    Ok(())
}

/// The splice VALUE's element coercion (CRuby `rb_ary_to_ary`): a plain
/// Array's own elements; a `to_ary`-defining object's coercion (TypeError
/// if it answers a non-Array, non-nil value); anything else as ONE element.
fn splice_elems(value: &RubyValue) -> Result<Vec<RubyValue>, crate::Signal> {
    if let RubyValue::Array(a) = value {
        return Ok(a.lock().to_vec());
    }
    let to_ary = crate::symbol::wk::to_ary();
    if crate::dispatch::responds_to(value.class_id(), to_ary, false) {
        match crate::dispatch::send_value(value, to_ary, &[], None)? {
            RubyValue::Array(a) => return Ok(a.lock().to_vec()),
            RubyValue::Nil => {}
            other => {
                return Err(type_error!(
                    "can't convert {} to Array ({}#to_ary gives {})",
                    crate::builtins::class_name_of(value),
                    crate::builtins::class_name_of(value),
                    crate::builtins::class_name_of(&other)
                ));
            }
        }
    }
    Ok(vec![value.clone()])
}

/// `index_only` -- `dig`'s per-level `[]` (an index through the `to_int`
/// protocol, as CRuby's `rb_ary_at` converts it).
fn index_only(recv: &RubyValue, key: &RubyValue) -> Result<RubyValue, crate::Signal> {
    let RubyValue::Array(a) = recv else {
        unreachable!("Array#dig dispatched on a non-Array receiver");
    };
    Ok(crate::array_get(a, convert::to_index(key)?))
}

/// `sort`/`sort!`'s comparator: the block when given, `rb_cmp` otherwise
/// (incomparable pairs raise real Ruby's ArgumentError).
pub(crate) fn sort_items(
    items: &mut [RubyValue],
    block: &Option<RubyValue>,
) -> Result<(), crate::Signal> {
    let mut failure: Option<crate::Signal> = None;
    if let Some(RubyValue::Proc(p)) = block {
        items.sort_by(|a, b| {
            if failure.is_some() {
                return std::cmp::Ordering::Equal;
            }
            match p.call(&[a.clone(), b.clone()]) {
                // The block's answer is validated like a `<=>` result (a
                // Float orders by sign; `nil` fails on the two elements; a
                // non-numeric fails as `comparison of <class> with 0`).
                Ok(r) => match crate::value::cmp_int(&r) {
                    Ok(Some(c)) => c.cmp(&0),
                    Ok(None) => {
                        failure = Some(crate::value::cmp_error(a, b));
                        std::cmp::Ordering::Equal
                    }
                    Err(e) => {
                        failure = Some(e);
                        std::cmp::Ordering::Equal
                    }
                },
                Err(e) => {
                    failure = Some(e);
                    std::cmp::Ordering::Equal
                }
            }
        });
    } else {
        items.sort_by(|a, b| {
            if failure.is_some() {
                return std::cmp::Ordering::Equal;
            }
            // Compare in array order (`a[i] <=> a[j]`, not the reverse
            // `sort_by` hands us) so an incomparable pair's ArgumentError
            // names the operands in CRuby's left-to-right order.
            match crate::value::cmp_or_raise(b, a) {
                Ok(c) => c.cmp(&0).reverse(),
                Err(e) => {
                    failure = Some(e);
                    std::cmp::Ordering::Equal
                }
            }
        });
    }
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// `select!`/`reject!`/`keep_if`/`delete_if`'s core: keeps elements whose
/// block result matches `keep`; answers self when anything changed, nil
/// otherwise.
fn in_place_filter(
    recv: &RubyValue,
    meth: &str,
    block: Option<RubyValue>,
    keep: bool,
) -> Result<RubyValue, crate::Signal> {
    let RubyValue::Array(handle) = recv else {
        unreachable!("Array table row dispatched on a non-Array receiver");
    };
    check_frozen(handle, recv)?;
    let p = block_or_enum!(recv, meth, &[], block);
    let items = handle.lock().clone();
    let mut out = Vec::with_capacity(items.len());
    for e in items.iter() {
        if p.call(std::slice::from_ref(e))?.truthy() == keep {
            out.push(e.clone());
        }
    }
    let changed = out.len() != items.len();
    *handle.lock() = out.into();
    Ok(if changed {
        recv.clone()
    } else {
        RubyValue::Nil
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::ARRAY_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    fn arr(vals: Vec<RubyValue>) -> RubyValue {
        RubyValue::Array(crate::array_new(vals))
    }

    #[test]
    fn push_is_variadic_and_returns_the_receiver() {
        let a = arr(vec![RubyValue::Int(1)]);
        imethod("<<")(&a, &[RubyValue::Int(2), RubyValue::Int(3)], None).unwrap();
        let RubyValue::Array(inner) = &a else {
            panic!()
        };
        assert_eq!(inner.lock().len(), 3);
    }

    fn items_of(v: &RubyValue) -> Vec<String> {
        let RubyValue::Array(inner) = v else {
            panic!("expected an Array")
        };

        inner.lock().iter().map(|e| e.inspect_string()).collect()
    }

    /// `a[start, len] = v` replaces the SPAN with `v`'s elements -- not the
    /// one-index store `a[i] = v`. Oracle-verified shapes; the
    /// `array_splice` example covers them end to end.
    #[test]
    fn index_set_splices_a_start_length_span() {
        let a = arr(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
            RubyValue::Int(4),
        ]);
        let repl = arr(vec![RubyValue::Int(8), RubyValue::Int(9)]);
        imethod("[]=")(&a, &[RubyValue::Int(1), RubyValue::Int(2), repl], None).unwrap();
        assert_eq!(items_of(&a), ["1", "8", "9", "4"]);
    }

    #[test]
    fn index_set_with_a_zero_length_inserts_without_removing() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        let repl = arr(vec![RubyValue::Int(9)]);
        imethod("[]=")(&a, &[RubyValue::Int(1), RubyValue::Int(0), repl], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9", "2"]);
    }

    #[test]
    fn index_set_splice_shrinks_when_the_replacement_is_shorter() {
        let a = arr(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
        ]);
        let repl = arr(vec![RubyValue::Int(9)]);
        imethod("[]=")(&a, &[RubyValue::Int(0), RubyValue::Int(3), repl], None).unwrap();
        assert_eq!(items_of(&a), ["9"]);
    }

    /// A non-Array value inserts as ONE element (no to_ary here).
    #[test]
    fn index_set_splice_inserts_a_scalar_as_one_element() {
        let a = arr(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
        ]);
        imethod("[]=")(
            &a,
            &[RubyValue::Int(0), RubyValue::Int(2), RubyValue::Int(9)],
            None,
        )
        .unwrap();
        assert_eq!(items_of(&a), ["9", "3"]);
    }

    #[test]
    fn index_set_splice_past_the_end_nil_pads_first() {
        let a = arr(vec![RubyValue::Int(1)]);
        let repl = arr(vec![RubyValue::Int(9)]);
        imethod("[]=")(&a, &[RubyValue::Int(3), RubyValue::Int(0), repl], None).unwrap();
        assert_eq!(items_of(&a), ["1", "nil", "nil", "9"]);
    }

    /// The expression VALUE is the right-hand side as written -- never the
    /// coercion, never the receiver.
    #[test]
    fn index_set_splice_answers_the_value_as_written() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        let repl = arr(vec![RubyValue::Int(9)]);
        let out = imethod("[]=")(&a, &[RubyValue::Int(0), RubyValue::Int(1), repl], None).unwrap();
        assert_eq!(out.inspect_string(), "[9]");
    }

    #[test]
    fn index_set_splices_an_inclusive_range() {
        let a = arr(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
            RubyValue::Int(4),
        ]);
        let range = crate::builtins::range::range_value(
            Some(RubyValue::Int(1)),
            Some(RubyValue::Int(2)),
            false,
        );
        imethod("[]=")(&a, &[range, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9", "4"]);
    }

    #[test]
    fn index_set_splices_an_exclusive_range() {
        let a = arr(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
            RubyValue::Int(4),
        ]);
        let range = crate::builtins::range::range_value(
            Some(RubyValue::Int(1)),
            Some(RubyValue::Int(3)),
            true,
        );
        imethod("[]=")(&a, &[range, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9", "4"]);
    }

    /// An endless range splices to the end; a beginless one from the start.
    #[test]
    fn index_set_splices_open_ended_ranges() {
        let a = arr(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
        ]);
        let endless = crate::builtins::range::range_value(Some(RubyValue::Int(1)), None, false);
        imethod("[]=")(&a, &[endless, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["1", "9"]);

        let b = arr(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
        ]);
        let beginless = crate::builtins::range::range_value(None, Some(RubyValue::Int(1)), false);
        imethod("[]=")(&b, &[beginless, RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&b), ["9", "3"]);
    }

    /// The plain one-index store still stores (no splice).
    #[test]
    fn index_set_with_two_args_and_an_int_index_stores_one_element() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        imethod("[]=")(&a, &[RubyValue::Int(0), RubyValue::Int(9)], None).unwrap();
        assert_eq!(items_of(&a), ["9", "2"]);
    }

    #[test]
    fn to_h_maps_each_element_through_a_block() {
        let a = arr(vec![RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
        ]))]);
        // `{ |pair| [pair[1], pair[0]] }` -- an Array yields ONE value per
        // element, so the block sees the element itself.
        let p: crate::RProc = crate::RProc::new(|args: &[RubyValue]| {
            let RubyValue::Array(pair) = &args[0] else {
                panic!("expected the element")
            };
            let pair = pair.lock().clone();
            Ok(RubyValue::Array(crate::array_new(vec![
                pair[1].clone(),
                pair[0].clone(),
            ])))
        });
        let out = imethod("to_h")(&a, &[], Some(RubyValue::Proc(p))).unwrap();
        assert_eq!(out.inspect_string(), "{2 => 1}");
    }

    #[test]
    fn to_h_without_a_block_requires_pair_shaped_elements() {
        let a = arr(vec![RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
        ]))]);
        let out = imethod("to_h")(&a, &[], None).unwrap();
        assert_eq!(out.inspect_string(), "{1 => 2}");
    }

    #[test]
    fn index_rejects_a_symbol_with_a_type_error_shape() {
        let a = arr(vec![RubyValue::Int(1)]);
        let sym = RubyValue::Symbol(crate::Symbol::intern("x"));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            imethod("[]")(&a, &[sym], None)
        }));
        assert!(r.is_err()); // registry-less: TypeError surfaces as a panic
    }

    #[test]
    fn each_yields_every_element_and_returns_the_receiver() {
        let a = arr(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        let seen = std::sync::Arc::new(parking_lot::Mutex::new(0i64));
        let seen2 = std::sync::Arc::clone(&seen);
        let p: crate::RProc = crate::RProc::new(move |args: &[RubyValue]| {
            if let RubyValue::Int(i) = &args[0] {
                *seen2.lock() += i;
            }
            Ok(RubyValue::Nil)
        });
        imethod("each")(&a, &[], Some(RubyValue::Proc(p))).unwrap();
        assert_eq!(*seen.lock(), 3);
    }
}
