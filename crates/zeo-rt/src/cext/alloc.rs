//! `ruby_xmalloc` and friends.
//!
//! An extension allocates its C struct with these and frees it with
//! `ruby_xfree`, and `RUBY_DEFAULT_FREE` means "this came from here". MRI
//! routes them through its own allocator so it can count bytes against the GC
//! trigger; zeo routes them through Rust's global allocator so the two halves
//! of a `ruby_xmalloc`/`ruby_xfree` pair agree, which is the only property
//! the extension can observe.
//!
//! The size is stored in a header word before the block, because `dealloc`
//! needs the `Layout` and `ruby_xfree` is handed only the pointer. That word
//! is also what makes `ruby_xrealloc` able to copy the right number of bytes.

use std::alloc::Layout;
use std::ffi::c_void;

/// The header is one `usize` and the payload is 16-aligned after it, which
/// covers every C type an extension can declare on zeo's targets.
const ALIGN: usize = 16;

fn layout(size: usize) -> Layout {
    Layout::from_size_align(size + ALIGN, ALIGN).expect("a C allocation size")
}

/// # Safety
///
/// The block must be freed with [`xfree`] and nothing else.
pub unsafe fn xmalloc(size: usize) -> *mut c_void {
    // SAFETY: the layout is non-zero because of the header.
    let base = unsafe { std::alloc::alloc(layout(size)) };
    if base.is_null() {
        // An extension has no way to recover, and MRI raises NoMemoryError
        // here. Aborting is louder and, at C0, honest about what zeo does.
        std::alloc::handle_alloc_error(layout(size));
    }
    // SAFETY: `base` is a fresh block of `size + ALIGN` bytes.
    unsafe {
        base.cast::<usize>().write(size);
        base.add(ALIGN).cast()
    }
}

/// # Safety
///
/// `p` must come from [`xmalloc`] or [`xrealloc`], or be null.
pub unsafe fn xcalloc(count: usize, size: usize) -> *mut c_void {
    let total = count.checked_mul(size).expect("a C allocation size");
    // SAFETY: same contract as `xmalloc`.
    let p = unsafe { xmalloc(total) };
    // SAFETY: `p` names `total` freshly allocated bytes.
    unsafe { std::ptr::write_bytes(p.cast::<u8>(), 0, total) };
    p
}

/// # Safety
///
/// `p` must come from [`xmalloc`] or [`xrealloc`], or be null.
pub unsafe fn xfree(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    // SAFETY: the caller's contract puts the header one ALIGN before `p`.
    unsafe {
        let base = p.cast::<u8>().sub(ALIGN);
        let size = base.cast::<usize>().read();
        std::alloc::dealloc(base, layout(size));
    }
}

/// # Safety
///
/// `p` must come from [`xmalloc`] or [`xrealloc`], or be null.
pub unsafe fn xrealloc(p: *mut c_void, size: usize) -> *mut c_void {
    if p.is_null() {
        // SAFETY: no prior block to honour.
        return unsafe { xmalloc(size) };
    }
    // SAFETY: the caller's contract.
    unsafe {
        let base = p.cast::<u8>().sub(ALIGN);
        let old = base.cast::<usize>().read();
        let grown = std::alloc::realloc(base, layout(old), size + ALIGN);
        if grown.is_null() {
            std::alloc::handle_alloc_error(layout(size));
        }
        grown.cast::<usize>().write(size);
        grown.add(ALIGN).cast()
    }
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
        pub unsafe extern "C" fn $name($($arg : $ty),*) -> $ret $body
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
        None => crate::cext::jmp::raise(crate::dispatch::raise_error(
            "NoMemoryError",
            format!("malloc: possible integer overflow ({count} * {size})"),
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

    #[test]
    fn a_block_is_aligned_for_any_c_type() {
        unsafe {
            let p = xmalloc(1);
            assert_eq!(p as usize % ALIGN, 0);
            xfree(p);
        }
    }
}
