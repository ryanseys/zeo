//! `ruby_xmalloc` and friends.
//!
//! An extension allocates its C struct with these and frees it with
//! `ruby_xfree`. MRI routes them through its own allocator so it can count
//! bytes against the GC trigger; zeo routes them straight to `malloc`.
//!
//! # Why libc's allocator and not Rust's
//!
//! Because the two allocators MUST be interchangeable, and Rust's cannot be.
//!
//! `ruby/util.h` does `#define strdup(s) ruby_strdup(s)` unconditionally --
//! upstream's own comment calls the idea unwise -- so a gem writing plain
//! `strdup` gets zeo's allocator and then writes `free(p)`. On MRI that
//! works: `ruby_xmalloc` IS `malloc` plus accounting, with no header.
//!
//! This used to wrap Rust's global allocator, which needs the `Layout` back
//! at `dealloc` and therefore needs the size stored somewhere -- a header
//! word before the payload. A libc `free` on that pointer is 16 bytes past
//! the real block, and libmalloc aborts. `bcrypt` did exactly that, at
//! `free(salt)` on the result of a `strdup` it never knew was rewritten, and
//! the abort named neither the gem nor the allocator.
//!
//! `malloc` needs no size to free, so there is no header, and the two
//! spellings are the same operation. That is the property an extension
//! assumes, and it is not one zeo gets to redefine.

use std::ffi::c_void;

/// # Safety
///
/// The block must be freed with [`xfree`] or libc's `free`, which are the
/// same operation -- see the module docs.
pub unsafe fn xmalloc(size: usize) -> *mut c_void {
    // A zero-size `malloc` may answer null, and MRI's own `ruby_xmalloc`
    // never does: an extension tests the result against null to mean
    // failure, and a legitimate empty allocation would read as one.
    // SAFETY: a plain allocation.
    let p = unsafe { libc::malloc(size.max(1)) };
    if p.is_null() {
        no_memory(size);
    }
    p
}

/// # Safety
///
/// The block must be freed with [`xfree`] or libc's `free`.
pub unsafe fn xcalloc(count: usize, size: usize) -> *mut c_void {
    // SAFETY: a plain allocation; `calloc` does the overflow check itself
    // and answers null, which is checked.
    let p = unsafe { libc::calloc(count.max(1), size.max(1)) };
    if p.is_null() {
        no_memory(count.saturating_mul(size));
    }
    p
}

/// # Safety
///
/// `p` must come from [`xmalloc`], [`xrealloc`] or libc's `malloc`, or be
/// null.
pub unsafe fn xfree(p: *mut c_void) {
    // SAFETY: the caller's contract. `free(NULL)` is defined and does
    // nothing, so the null case needs no branch of its own.
    unsafe { libc::free(p) };
}

/// # Safety
///
/// `p` must come from [`xmalloc`], [`xrealloc`] or libc's `malloc`, or be
/// null.
pub unsafe fn xrealloc(p: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: the caller's contract. `realloc(NULL, n)` is `malloc(n)`.
    let out = unsafe { libc::realloc(p, size.max(1)) };
    if out.is_null() {
        no_memory(size);
    }
    out
}

/// An allocation an extension has no way to recover from. MRI raises
/// `NoMemoryError`; zeo raises the same, so a `rescue NoMemoryError` around
/// a big allocation still works.
fn no_memory(size: usize) -> ! {
    crate::unwind::raise(zeo_rt::builtins::no_memory_error!(
        "failed to allocate {size} bytes"
    ))
}

/// The five names C spells. `xmalloc` and its neighbours are the
/// implementation; these are what a gem links against, and a missing one
/// would send every allocation to a loud stub rather than the allocator.
///
/// `ruby_xmalloc2(n, size)` and `ruby_xrealloc2` multiply, and MRI raises
/// rather than wrapping -- an overflow there is how a short buffer gets
/// written past.
macro_rules! alloc_fn {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty $body:block
    )*) => {$(
        $(#[$meta])*
        #[unsafe(no_mangle)]
        // A C ABI entry point with MRI's own contract, stated per row above.
        #[allow(clippy::missing_safety_doc)]
        pub unsafe extern "C-unwind" fn $name($($arg : $ty),*) -> $ret $body
    )*};
}

alloc_fn! {
    fn ruby_xmalloc(size: usize) -> *mut c_void {
        // SAFETY: the caller frees it with `ruby_xfree`, which is MRI's
        // contract for this entry too.
        unsafe { xmalloc(size) }
    }

    fn ruby_xmalloc2(count: usize, size: usize) -> *mut c_void {
        // SAFETY: as above.
        unsafe { xmalloc(checked(count, size)) }
    }

    fn ruby_xcalloc(count: usize, size: usize) -> *mut c_void {
        // SAFETY: as above.
        unsafe { xcalloc(count, size) }
    }

    fn ruby_xrealloc(p: *mut c_void, size: usize) -> *mut c_void {
        // SAFETY: the caller's contract -- `p` came from one of these.
        unsafe { xrealloc(p, size) }
    }

    fn ruby_xrealloc2(p: *mut c_void, count: usize, size: usize) -> *mut c_void {
        // SAFETY: as above.
        unsafe { xrealloc(p, checked(count, size)) }
    }

    fn ruby_xfree(p: *mut c_void) -> () {
        // SAFETY: as above.
        unsafe { xfree(p) };
    }
}

/// `count * size`, or the raise MRI makes. A wrapped product is a buffer
/// shorter than the caller believes and a write past its end.
fn checked(count: usize, size: usize) -> usize {
    match count.checked_mul(size) {
        Some(n) => n,
        None => crate::unwind::raise(zeo_rt::builtins::no_memory_error!(
            "malloc: possible integer overflow ({count} * {size})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_round_trips_and_keeps_its_bytes() {
        unsafe {
            let p = xmalloc(64).cast::<u8>();
            for i in 0..64u8 {
                p.add(i as usize).write(i);
            }
            let p = xrealloc(p.cast(), 256).cast::<u8>();
            for i in 0..64u8 {
                assert_eq!(p.add(i as usize).read(), i, "realloc lost byte {i}");
            }
            xfree(p.cast());
        }
    }

    #[test]
    fn xcalloc_zeroes_and_a_null_free_is_a_no_op() {
        unsafe {
            let p = xcalloc(8, 8).cast::<u8>();
            assert!((0..64).all(|i| p.add(i).read() == 0));
            xfree(p.cast());
            xfree(std::ptr::null_mut());
        }
    }

    /// A zero-size allocation must not answer null: an extension tests the
    /// result against null to mean FAILURE, and MRI's own never answers one.
    #[test]
    fn a_zero_size_allocation_is_not_null() {
        unsafe {
            let p = xmalloc(0);
            assert!(!p.is_null());
            xfree(p);
            let p = xcalloc(0, 0);
            assert!(!p.is_null());
            xfree(p);
        }
    }

    /// The property the whole file exists for: a block from `ruby_xmalloc`
    /// is freeable by libc's `free`, and one from libc's `malloc` is
    /// freeable by `ruby_xfree`. `ruby/util.h` rewrites plain `strdup` into
    /// `ruby_strdup` unconditionally, so a gem crosses the two without ever
    /// writing either name -- and a header word in front made that abort.
    #[test]
    fn the_two_allocators_are_one() {
        unsafe {
            let ours = xmalloc(32);
            libc::free(ours);

            let theirs = libc::malloc(32);
            xfree(theirs);
        }
    }

    /// `ruby_strdup`'s result reaches libc's `free`, which is the exact path
    /// `bcrypt` takes through `crypt_gensalt_ra`.
    #[test]
    fn a_ruby_strdup_result_is_freeable_by_libc() {
        let src = c"a salt-shaped string";
        // SAFETY: a NUL-terminated literal.
        let dup = unsafe { ruby_strdup_for_test(src.as_ptr()) };
        // SAFETY: the copy is NUL-terminated by construction.
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(dup) }, src);
        // SAFETY: the whole point -- libc frees what zeo allocated.
        unsafe { libc::free(dup.cast()) };
    }

    /// `ruby_strdup`'s body, reachable without the `cext_fn!` wrapper's
    /// longjmp -- which a unit test has no protected frame for.
    ///
    /// # Safety
    ///
    /// `s` must be NUL-terminated.
    unsafe fn ruby_strdup_for_test(s: *const std::ffi::c_char) -> *mut std::ffi::c_char {
        // SAFETY: the caller's contract.
        let len = unsafe { std::ffi::CStr::from_ptr(s) }.to_bytes().len();
        // SAFETY: `len + 1` bytes, freed by the caller.
        let out = unsafe { xmalloc(len + 1) }.cast::<u8>();
        // SAFETY: both runs are `len` bytes and `out` has room for the NUL.
        unsafe {
            std::ptr::copy_nonoverlapping(s.cast::<u8>(), out, len);
            out.add(len).write(0);
        }
        out.cast()
    }
}
