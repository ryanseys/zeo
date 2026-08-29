//! The two orderings CRuby sorts with, ported so a tie lands where ruby's
//! does.
//!
//! Ruby documents `sort`'s order for equal elements as indeterminate, but a
//! golden records the answer all the same, so "indeterminate" is not a thing
//! zeo can choose for itself. CRuby reaches that answer two ways and zeo has
//! to reach it the same two ways:
//!
//! * `ruby_qsort` -- the SYSTEM `qsort_r`, on every platform zeo ships. The
//!   tie order is the C library's, so calling the same routine is the only
//!   way to land on it.
//! * `rb_uniform_intro_sort_2` -- ruby's OWN introsort, which `sort_by` uses
//!   in place of `qsort` when every key is a fixnum or a Float. It is a
//!   different permutation entirely, so `uniform_bits` below decides which of
//!   the two runs, exactly as `sort_by_i` does.

use crate::RubyValue;
use crate::Signal;

/// The comparator, erased so one trampoline serves every call site.
type SortCmp<'a, T> = &'a mut dyn FnMut(&T, &T) -> std::cmp::Ordering;

/// `ruby_qsort`: the system `qsort_r`. libc moves elements by raw bytes,
/// which is what a Rust move is, and runs no destructor, so a `RubyValue`
/// survives the shuffle untouched.
pub(crate) fn ruby_qsort<T>(items: &mut [T], mut cmp: impl FnMut(&T, &T) -> std::cmp::Ordering) {
    if items.len() < 2 {
        return;
    }
    let mut erased: SortCmp<T> = &mut cmp;
    let arg: *mut std::ffi::c_void = std::ptr::from_mut(&mut erased).cast();
    let (base, num) = (items.as_mut_ptr().cast(), items.len());
    let size = size_of::<T>();
    // SAFETY: `base`/`num`/`size` describe `items` exactly, and `arg` outlives
    // the call. The trampoline is `extern "C"`, so a panic inside the
    // comparator aborts rather than unwinding through C.
    unsafe {
        #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
        libc::qsort_r(base, num, size, arg, Some(sort_trampoline::<T>));
        #[cfg(not(any(target_vendor = "apple", target_os = "freebsd")))]
        libc::qsort_r(base, num, size, Some(sort_trampoline::<T>), arg);
    }
}

#[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
unsafe extern "C" fn sort_trampoline<T>(
    arg: *mut std::ffi::c_void,
    a: *const std::ffi::c_void,
    b: *const std::ffi::c_void,
) -> std::ffi::c_int {
    unsafe { sort_call::<T>(arg, a, b) }
}

#[cfg(not(any(target_vendor = "apple", target_os = "freebsd")))]
unsafe extern "C" fn sort_trampoline<T>(
    a: *const std::ffi::c_void,
    b: *const std::ffi::c_void,
    arg: *mut std::ffi::c_void,
) -> std::ffi::c_int {
    unsafe { sort_call::<T>(arg, a, b) }
}

unsafe fn sort_call<T>(
    arg: *mut std::ffi::c_void,
    a: *const std::ffi::c_void,
    b: *const std::ffi::c_void,
) -> std::ffi::c_int {
    // SAFETY: `ruby_qsort` handed libc a `*mut SortCmp<T>` and two pointers
    // into the slice it is sorting; libc hands all three back unchanged.
    let (cmp, a, b) = unsafe {
        (
            &mut *arg.cast::<SortCmp<T>>(),
            &*a.cast::<T>(),
            &*b.cast::<T>(),
        )
    };
    match cmp(a, b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// A `sort_by` key/element pair, laid out as CRuby's `rb_uniform_sort_data`.
type Decorated = (RubyValue, RubyValue);

/// `rb_nmin_run`: what `min(n)`, `max(n)`, `min_by(n)` and `max_by(n)` answer.
///
/// NOT "sort everything and take n". CRuby buffers `4n` elements, then
/// QUICKSELECTS them down to `n` and remembers the last pivot as a LIMIT --
/// after which an element no better than the limit is dropped without being
/// buffered at all. So the SET of survivors among equal keys, not just their
/// order, is decided by where quickselect's partition happened to land, and
/// only the survivors are sorted at the end.
pub(crate) struct Nmin<F> {
    n: usize,
    /// `max`/`max_by`: the comparison is read backwards.
    rev: bool,
    /// `(key, element)`. Without a `_by` block the key IS the element, which
    /// is what makes one buffer serve both -- CRuby's `GETPTR` points at the
    /// key slot either way.
    buf: Vec<Decorated>,
    limit: Option<RubyValue>,
    cmp: F,
}

impl<F: FnMut(&RubyValue, &RubyValue) -> Result<i32, Signal>> Nmin<F> {
    pub(crate) fn new(n: usize, rev: bool, cmp: F) -> Self {
        Nmin {
            n,
            rev,
            buf: Vec::with_capacity(n.saturating_mul(4)),
            limit: None,
            cmp,
        }
    }

    /// One element, with its key. Answers once the buffer has been filtered,
    /// which is where the limit is set.
    pub(crate) fn push(&mut self, key: RubyValue, elem: RubyValue) -> Result<(), Signal> {
        if self.n == 0 {
            return Ok(());
        }
        if let Some(limit) = self.limit.clone() {
            let c = self.ordered(&key, &limit)?;
            if c >= 0 {
                return Ok(());
            }
        }
        self.buf.push((key, elem));
        if self.buf.len() == self.n * 4 {
            self.filter()?;
        }
        Ok(())
    }

    /// The survivors, in CRuby's order: sorted, then reversed for `max`.
    pub(crate) fn finish(mut self) -> Result<Vec<RubyValue>, Signal> {
        self.filter()?;
        let mut failure: Option<Signal> = None;
        let (cmp, rev) = (&mut self.cmp, self.rev);
        ruby_qsort(&mut self.buf, |a, b| {
            if failure.is_some() {
                return std::cmp::Ordering::Equal;
            }
            match cmp(&a.0, &b.0) {
                Ok(c) => c.cmp(&0),
                Err(e) => {
                    failure = Some(e);
                    std::cmp::Ordering::Equal
                }
            }
        });
        if let Some(e) = failure {
            return Err(e);
        }
        let mut out: Vec<RubyValue> = self.buf.into_iter().map(|(_, e)| e).collect();
        if rev {
            out.reverse();
        }
        Ok(out)
    }

    fn ordered(&mut self, a: &RubyValue, b: &RubyValue) -> Result<i32, Signal> {
        let c = (self.cmp)(a, b)?;
        Ok(if self.rev { -c } else { c })
    }

    /// `nmin_filter`: quickselect the buffer down to `n`, pivot at the
    /// midpoint, with equal elements gathered at the right and moved back to
    /// the middle. The LAST PIVOT becomes the limit.
    fn filter(&mut self) -> Result<(), Signal> {
        if self.buf.len() <= self.n {
            return Ok(());
        }
        let (mut left, mut right) = (0isize, self.buf.len() as isize - 1);
        let n = self.n as isize;
        let store_index = loop {
            let mut pivot = left + (right - left) / 2;
            let mut num_pivots = 1isize;
            self.buf.swap(pivot as usize, right as usize);
            pivot = right;

            let mut store_index = left;
            let mut i = left;
            while i <= right - num_pivots {
                let c = {
                    let (a, b) = (self.buf[i as usize].0.clone(), self.buf[pivot as usize].0.clone());
                    self.ordered(&a, &b)?
                };
                if c == 0 {
                    self.buf.swap(i as usize, (right - num_pivots) as usize);
                    num_pivots += 1;
                    continue;
                }
                if c < 0 {
                    self.buf.swap(i as usize, store_index as usize);
                    store_index += 1;
                }
                i += 1;
            }
            // The equal run sits at the far right; walk it back so it lands
            // just past the elements that sorted before the pivot.
            let mut j = store_index;
            let mut i = right;
            while right - num_pivots < i {
                if i <= j {
                    break;
                }
                self.buf.swap(j as usize, i as usize);
                j += 1;
                i -= 1;
            }
            if store_index <= n && n <= store_index + num_pivots {
                break store_index;
            }
            if n < store_index {
                right = store_index - 1;
            } else {
                left = store_index + num_pivots;
            }
        };
        self.limit = Some(self.buf[store_index as usize].0.clone());
        self.buf.truncate(self.n);
        Ok(())
    }
}

/// CRuby's `SORT_BY_UNIFORMED` bits for one key: `num` (fixnum or Float),
/// `flo`, `fix`. `sort_by_i` ANDs these across every key, and the uniform
/// sort runs when anything survives.
pub(crate) fn uniform_bits(v: &RubyValue) -> u8 {
    // CRuby's FIXNUM is 63-bit, so an `Int` past that is a Bignum to ruby and
    // takes the `qsort` path with everything else that is not a raw number.
    let fix = matches!(v, RubyValue::Int(i) if (-(1 << 62)..1 << 62).contains(i));
    let flo = matches!(v, RubyValue::Float(_));
    (u8::from(fix || flo) << 2) | (u8::from(flo) << 1) | u8::from(fix)
}

/// The comparison state the sort carries: CRuby raises out of the middle of
/// the sort when a NaN turns up, so zeo keeps the first failure and hands the
/// half-sorted array back to be discarded.
struct Uniform {
    failure: Option<Signal>,
}

impl Uniform {
    /// `rb_uniform_is_less`. The operand order below is the one CRuby hands
    /// `rb_float_cmp`, so a NaN's message names the same two values.
    fn less(&mut self, a: &RubyValue, b: &RubyValue) -> bool {
        match (a, b) {
            (RubyValue::Int(x), RubyValue::Int(y)) => x < y,
            (RubyValue::Int(_), _) => self.cmp(b, a).is_some_and(|c| c > 0),
            _ => self.cmp(a, b).is_some_and(|c| c < 0),
        }
    }

    /// `rb_uniform_is_larger`.
    fn larger(&mut self, a: &RubyValue, b: &RubyValue) -> bool {
        match (a, b) {
            (RubyValue::Int(x), RubyValue::Int(y)) => x > y,
            (RubyValue::Int(_), _) => self.cmp(b, a).is_some_and(|c| c < 0),
            _ => self.cmp(a, b).is_some_and(|c| c > 0),
        }
    }

    fn cmp(&mut self, a: &RubyValue, b: &RubyValue) -> Option<i64> {
        if self.failure.is_some() {
            return None;
        }
        match a.rb_cmp(b) {
            Some(c) => Some(c),
            None => {
                self.failure = Some(crate::value::cmp_error(a, b));
                None
            }
        }
    }
}

/// `rb_uniform_intro_sort_2`. Answers the comparison error CRuby would have
/// raised from inside the sort; the half-sorted array goes with it.
pub(crate) fn uniform_intro_sort(items: &mut [Decorated]) -> Result<(), Signal> {
    let mut u = Uniform { failure: None };
    let n = items.len();
    if n < 2 {
        return Ok(());
    }
    // Already non-decreasing -- CRuby leaves the array alone, which makes the
    // uniform path stable for input that is already in order.
    let mut sorted = true;
    for i in 1..n {
        if u.larger(&items[i - 1].0, &items[i].0) {
            sorted = false;
            break;
        }
    }
    if let Some(e) = u.failure {
        return Err(e);
    }
    if sorted {
        return Ok(());
    }
    let d = (usize::BITS - 1 - n.leading_zeros()) as usize;
    quicksort_intro(items, d << 1, &mut u);
    match u.failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// `med3_val`.
fn med3(a: &RubyValue, b: &RubyValue, c: &RubyValue, u: &mut Uniform) -> RubyValue {
    let pick = if u.less(a, b) {
        if u.less(b, c) {
            b
        } else if u.less(c, a) {
            a
        } else {
            c
        }
    } else if u.less(c, b) {
        b
    } else if u.less(a, c) {
        a
    } else {
        c
    };
    pick.clone()
}

/// `rb_uniform_insertionsort_2`. CRuby shifts a hole down the array; the
/// rotate below moves the same element to the same place after the same
/// comparisons.
fn insertion_sort(items: &mut [Decorated], u: &mut Uniform) {
    for index in 1..items.len() {
        let pos = if u.less(&items[index].0, &items[0].0) {
            0
        } else {
            let mut k = index;
            loop {
                k -= 1;
                if !u.less(&items[index].0, &items[k].0) {
                    break k + 1;
                }
            }
        };
        items[pos..=index].rotate_right(1);
    }
}

/// `rb_uniform_heap_down_2`, over a heap whose live region is `..=len`.
fn heap_down(items: &mut [Decorated], mut offset: usize, len: usize, u: &mut Uniform) {
    let tmp = items[offset].clone();
    loop {
        let mut c = (offset << 1) + 1;
        if c > len {
            break;
        }
        if c < len && u.less(&items[c].0, &items[c + 1].0) {
            c += 1;
        }
        if !u.less(&tmp.0, &items[c].0) {
            break;
        }
        items[offset] = items[c].clone();
        offset = c;
    }
    items[offset] = tmp;
}

/// `rb_uniform_heapsort_2`.
fn heapsort(items: &mut [Decorated], u: &mut Uniform) {
    let n = items.len();
    if n < 2 {
        return;
    }
    let mut offset = n >> 1;
    while offset > 0 {
        offset -= 1;
        heap_down(items, offset, n - 1, u);
    }
    let mut offset = n - 1;
    while offset > 0 {
        items.swap(0, offset);
        offset -= 1;
        heap_down(items, 0, offset, u);
    }
}

/// `rb_uniform_quicksort_intro_2`.
fn quicksort_intro(items: &mut [Decorated], d: usize, u: &mut Uniform) {
    let n = items.len();
    if n <= 16 {
        insertion_sort(items, u);
        return;
    }
    if d == 0 {
        heapsort(items, u);
        return;
    }
    let x = med3(&items[0].0, &items[n >> 1].0, &items[n - 1].0, u);
    // `j` walks off the front when the pivot is the first element, exactly as
    // CRuby's pointer does, so the cursors are signed.
    // The pivot is one of the three sampled elements, so each scan meets it
    // and stops; the bounds are belt and braces against a comparator that is
    // not a strict weak ordering, where CRuby would read off the array.
    let (mut i, mut j) = (0isize, n as isize - 1);
    loop {
        while i < n as isize && u.less(&items[i as usize].0, &x) {
            i += 1;
        }
        while j >= 0 && u.less(&x, &items[j as usize].0) {
            j -= 1;
        }
        if i > j {
            break;
        }
        items.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
        if i > j {
            break;
        }
    }
    let (i, j) = (i as usize, (j + 1) as usize);
    if n - j > 1 {
        quicksort_intro(&mut items[j..], d - 1, u);
    }
    if i > 1 {
        quicksort_intro(&mut items[..i], d - 1, u);
    }
}
