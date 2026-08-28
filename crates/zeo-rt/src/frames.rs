//! The runtime CALL-FRAME stack backing `Exception#backtrace`,
//! `Kernel#caller`, and CRuby-shaped uncaught-exception reports.
//!
//! Generated method prologues push one lightweight [`Frame`] through a
//! [`FrameGuard`] (an RAII zero-sized type: `Drop` pops on EVERY exit
//! path, including `?`-propagated signals -- Rust runs drops on early
//! `return`, so no unwind machinery is needed in this `Result`-based
//! control-flow world). Statement emission stamps the CURRENT frame's line
//! (`set_line`) whenever the source line changes, so a captured backtrace
//! shows each frame at the line it was actually executing -- CRuby's own
//! per-frame PC-to-line reporting, at statement granularity.
//!
//! Frame text is baked at compile time (`&'static str` file names and
//! `'Class#method'` labels from the span tables), so a push is three stores
//! and a pointer bump, and the common no-raise path never formats anything.
//! Backtraces are FORMATTED at capture (raise) time.
//!
//! # Why the stack is split in two
//!
//! A thread-local whose TYPE needs dropping registers a destructor and
//! checks for it on every single access, and that check -- not the `Vec`
//! bookkeeping it was hiding behind -- was most of the old cost. Measured
//! on an M-series laptop, push+pop: `RefCell<Vec<Frame>>` 4.48 ns, the same
//! bump-pointer stack behind a drop-needing TLS 4.06 ns, and a
//! `Cell<*mut Frame>` trio that owns nothing 0.84 ns. `set_line` moves
//! 0.74 ns -> 0.37 ns the same way. So [`STACK`], which every call touches,
//! holds only raw pointers and has no `Drop`; [`OWNER`], which owns the
//! buffer so a finished thread frees it, is touched only by `grow` and
//! `swap_stack`.
//!
//! Thread-local, and each Ruby `Thread` is its own OS thread -- a raise in
//! one `Thread` never sees another's frames, by construction. Fibers swap
//! in their OWN frame stack via the ec-swap (`crate::ec`), so a raise
//! inside a fiber backtraces only the fiber's frames -- CRuby's own
//! per-fiber stack semantics, oracle-verified.

use std::cell::{Cell, RefCell};
use std::ptr;

/// One executing method activation -- everything a backtrace line needs.
///
/// `repr(C)`: emitted prologues write frames DIRECTLY through
/// [`zeo_rt_frame_hot`](crate::capi::frames::zeo_rt_frame_hot)'s pointer,
/// so the field offsets are ABI (`zeo_abi::abi::FRAME_*`), pinned by
/// `frame_hot_layout` below. The `&'static str` fields assume the (ptr,
/// len) fat-pointer layout; the same test pins that assumption loudly.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Frame {
    pub file: &'static str,
    /// The METHOD text, in one of two states. `method_ptr` non-null: a
    /// real `&'static str` in (ptr, len) halves -- what emitted
    /// prologues store. Null: the cold builtin-dispatch boundary's
    /// (owner class, symbol, separator) packed into `method_len`
    /// ([`Frame::pack_ids`]) -- the label is materialized only when the
    /// frame is READ (a capture, a trace event), which is what lets
    /// `with_c_frame_ids` skip the label intern's mutex per call.
    method_ptr: *const u8,
    method_len: usize,
    pub line: u32,
    /// The scope's `end` keyword line, `TracePoint`'s `:return`/`:end`
    /// lineno. 0 marks a frame that never fires entry/exit trace events
    /// (`<main>`, blocks, synthetic C frames).
    pub end_line: u32,
    /// The release-pool watermark this frame's pop drains to, or
    /// [`Frame::NO_MARK`] for a frame that brackets no pool scope (a
    /// synthetic C frame, a Rust-side `FrameGuard`). Rode a parallel
    /// `marks` stack while the rustc backend shared the frame struct;
    /// that constraint died with the backend.
    pub pool_mark: u32,
}

// SAFETY: `method_ptr` is null or points into process-lifetime text
// (`.rodata`, an interner leak); frames cross threads only inside the
// fiber ec-swap's `Vec<Frame>`.
unsafe impl Send for Frame {}
unsafe impl Sync for Frame {}

/// A frame method's two spellings, for the pushes that can carry either.
#[derive(Clone, Copy)]
pub(crate) enum FrameMethod {
    Label(&'static str),
    /// Owner class id + method symbol + "class method" (a `.` label
    /// rather than `#`).
    Ids(crate::ClassId, crate::Symbol, bool),
}

impl Frame {
    /// `pool_mark`'s "no pool scope" sentinel.
    pub const NO_MARK: u32 = zeo_abi::abi::FRAME_NO_MARK;

    const EMPTY: Frame = Frame::with_label("", "", 0, 0, Frame::NO_MARK);

    const fn with_label(
        file: &'static str,
        method: &'static str,
        line: u32,
        end_line: u32,
        pool_mark: u32,
    ) -> Frame {
        Frame {
            file,
            method_ptr: method.as_ptr(),
            method_len: method.len(),
            line,
            end_line,
            pool_mark,
        }
    }

    /// The ids state's packing: owner in the high half, the symbol
    /// shifted over the separator bit. A symbol id above 2^31 would
    /// collide with the owner half; the interner never gets there.
    fn pack_ids(owner: crate::ClassId, sym: crate::Symbol, class_sep: bool) -> usize {
        let s = sym.to_u32();
        debug_assert!(s < 1 << 31, "symbol id overflows the frame packing");
        ((owner.0 as usize) << 32) | ((s as usize) << 1) | usize::from(class_sep)
    }

    fn of(method: FrameMethod, file: &'static str, line: u32, end_line: u32) -> Frame {
        match method {
            FrameMethod::Label(l) => Frame::with_label(file, l, line, end_line, Frame::NO_MARK),
            FrameMethod::Ids(owner, sym, class_sep) => Frame {
                file,
                method_ptr: std::ptr::null(),
                method_len: Frame::pack_ids(owner, sym, class_sep),
                line,
                end_line,
                pool_mark: Frame::NO_MARK,
            },
        }
    }

    fn ids_of(&self) -> Option<(crate::ClassId, crate::Symbol, bool)> {
        if !self.method_ptr.is_null() {
            return None;
        }
        let owner = crate::ClassId((self.method_len >> 32) as u32);
        let sym = crate::Symbol::from_u32(((self.method_len >> 1) & 0x7FFF_FFFF) as u32);
        Some((owner, sym, self.method_len & 1 == 1))
    }

    /// The method text, materializing an ids-state frame's label through
    /// the intern cache (a READ is a capture or a trace event -- cold).
    pub fn method(&self) -> &'static str {
        match self.ids_of() {
            None => {
                // SAFETY: non-null ptr/len came from a real &'static str.
                unsafe {
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        self.method_ptr,
                        self.method_len,
                    ))
                }
            }
            Some((owner, sym, class_sep)) => {
                let sep = if class_sep { '.' } else { '#' };
                // The push-side NOFRAME/SPECIALIZED checks passed (or no
                // frame would exist), and both sets are static, so the
                // label materializes deterministically.
                crate::dispatch::c_frame_label(owner, sym, sep).unwrap_or("?")
            }
        }
    }

    /// Whether this frame's method IS `method`, across the two states --
    /// the synthetic-frame dedupe's question, answered without
    /// materializing a label (the cross-state arm compares piecewise).
    fn method_is(&self, method: FrameMethod) -> bool {
        match (self.ids_of(), method) {
            (None, FrameMethod::Label(l)) => {
                // Identity first (`ptr::eq` on the halves covers address
                // AND len): a repeat through one cached label is the
                // common hit. Content eq stays as the fallback.
                (std::ptr::eq(self.method_ptr, l.as_ptr()) && self.method_len == l.len())
                    || self.method() == l
            }
            (Some((o, s, c)), FrameMethod::Ids(o2, s2, c2)) => o == o2 && s == s2 && c == c2,
            (Some((o, s, c)), FrameMethod::Label(l)) => ids_match_label(o, s, c, l),
            (None, FrameMethod::Ids(o, s, c)) => ids_match_label(o, s, c, self.method()),
        }
    }
}

/// The cross-state dedupe compare: does `'Owner#name'` (or `'Owner.name'`)
/// spell exactly these ids? Piecewise -- no label materializes.
fn ids_match_label(
    owner: crate::ClassId,
    sym: crate::Symbol,
    class_sep: bool,
    label: &str,
) -> bool {
    let name = sym.name_str();
    let sep = if class_sep { '.' } else { '#' };
    let Some(rest) = label.strip_suffix(name) else {
        return false;
    };
    let Some(cls) = rest.strip_suffix(sep) else {
        return false;
    };
    crate::dispatch::class_name(owner).is_some_and(|n| n == cls)
}

/// The per-thread HOT header: the frame-stack trio and the release-pool
/// value trio, `base <= top <= end` into [`OWNER`]'s buffers, all null
/// before the first push. Deliberately owns nothing -- see the module
/// docs for the measurement that forces this. `repr(C)` because emitted
/// code addresses it directly (offsets in `zeo_abi::abi::FRAMEHOT_*`);
/// `Cell<*mut T>` has `*mut T`'s layout.
#[repr(C)]
pub struct FrameHot {
    pub(crate) top: Cell<*mut Frame>,
    pub(crate) base: Cell<*mut Frame>,
    pub(crate) end: Cell<*mut Frame>,
    pub(crate) pool_top: Cell<*mut crate::RubyValue>,
    pub(crate) pool_base: Cell<*mut crate::RubyValue>,
    pub(crate) pool_end: Cell<*mut crate::RubyValue>,
}

impl FrameHot {
    #[inline]
    fn len(&self) -> usize {
        let base = self.base.get();
        if base.is_null() {
            return 0;
        }
        // SAFETY: `top` and `base` index the same buffer, `top >= base`.
        unsafe { self.top.get().offset_from(base) as usize }
    }

    fn detach(&self) {
        self.top.set(ptr::null_mut());
        self.base.set(ptr::null_mut());
        self.end.set(ptr::null_mut());
    }
}

std::thread_local!(pub(crate) static STACK: FrameHot = const {
    FrameHot {
        top: Cell::new(ptr::null_mut()),
        base: Cell::new(ptr::null_mut()),
        end: Cell::new(ptr::null_mut()),
        pool_top: Cell::new(ptr::null_mut()),
        pool_base: Cell::new(ptr::null_mut()),
        pool_end: Cell::new(ptr::null_mut()),
    }
});

/// The cold half: owns the frame buffer so a finished thread frees it.
/// (The pool half's owner lives in `release_pool` and detaches its own
/// trio the same way.)
struct Owner(Vec<Frame>);

impl Drop for Owner {
    fn drop(&mut self) {
        // Teardown order between two thread-locals is unspecified, so cut
        // the hot pointers loose before the buffer goes. A push after this
        // point then finds an empty stack and is dropped (see `grow`)
        // rather than writing into freed memory.
        let _ = STACK.try_with(FrameHot::detach);
    }
}

std::thread_local!(static OWNER: RefCell<Owner> = const {
    RefCell::new(Owner(Vec::new()))
});

/// Room for at least one more frame. Runs once per thread, then once per
/// doubling of peak recursion depth -- never on an ordinary call, hence
/// `#[cold]`, which also keeps `push`'s inlined body down to the bump.
#[cold]
#[inline(never)]
fn grow() {
    // Snapshot through the raw pointers BEFORE borrowing the owner, so the
    // read and the reallocation never alias.
    let live: Vec<Frame> = with_frames(|f| f.to_vec());
    let want = (live.len() + 1).next_power_of_two().max(256);

    // During thread teardown `OWNER` may already be gone. Frames pushed
    // then are unobservable -- nothing left can capture a backtrace -- so
    // dropping the push is both harmless and the only answer that cannot
    // write into a freed buffer.
    let _ = OWNER.try_with(|owner| {
        let mut owner = owner.borrow_mut();
        owner.0 = vec![Frame::EMPTY; want];
        owner.0[..live.len()].copy_from_slice(&live);
        // Moving a `Vec` does not move its heap buffer, so this pointer
        // stays valid for as long as `owner.0` is not reassigned -- which
        // only this function does.
        let base = owner.0.as_mut_ptr();
        STACK.with(|s| {
            s.base.set(base);
            // SAFETY: `want > live.len()`, both inside the fresh buffer.
            s.top.set(unsafe { base.add(live.len()) });
            s.end.set(unsafe { base.add(want) });
        });
    });
}

#[inline]
fn push_frame(fr: Frame) {
    let pushed = STACK.with(|s| {
        let top = s.top.get();
        // Also catches the null/null initial state, which routes to `grow`.
        if top == s.end.get() {
            return false;
        }
        // SAFETY: `top < end`, so it addresses a live slot in the buffer.
        unsafe { top.write(fr) };
        s.top.set(unsafe { top.add(1) });
        true
    });
    if !pushed {
        grow_and_push(fr);
    }
}

#[cold]
#[inline(never)]
fn grow_and_push(fr: Frame) {
    grow();
    STACK.with(|s| {
        let top = s.top.get();
        if top.is_null() || top == s.end.get() {
            return; // teardown -- see `grow`
        }
        // SAFETY: as in `push_frame`.
        unsafe { top.write(fr) };
        s.top.set(unsafe { top.add(1) });
    });
}

/// Pop, reading back only the popped frame's `pool_mark` (or
/// [`Frame::NO_MARK`] on an empty stack). The old code's `Drop` moved the
/// whole popped `Frame` out unconditionally and that cost bm_fib ~8%;
/// only the tracing path needs more than the mark.
#[inline]
fn pop_frame_discard() -> u32 {
    STACK.with(|s| {
        let top = s.top.get();
        if top != s.base.get() {
            // SAFETY: `top > base`, so `top - 1` is a live slot.
            let top = unsafe { top.sub(1) };
            s.top.set(top);
            unsafe { (*top).pool_mark }
        } else {
            Frame::NO_MARK
        }
    })
}

/// The live frames as a slice, outermost first.
fn with_frames<R>(f: impl FnOnce(&[Frame]) -> R) -> R {
    STACK.with(|s| {
        let base = s.base.get();
        let len = s.len();
        // `from_raw_parts` rejects a null base even at length 0.
        let ptr = if base.is_null() {
            ptr::NonNull::dangling().as_ptr()
        } else {
            base
        };
        // SAFETY: `base..base+len` is the initialized prefix of the buffer,
        // which nothing else aliases while `f` runs -- `f` never pushes.
        f(unsafe { std::slice::from_raw_parts(ptr, len) })
    })
}

/// Install `new` as this context's frame stack, returning the previous one
/// -- the fiber ec-swap's slice of this cell (see `crate::ec`).
///
/// Copies, where the old `Vec`-backed stack could hand the buffer over
/// whole. A fiber switch is a coroutine stack switch either side of this
/// call, so a memcpy of the live frames does not register; an ordinary
/// method call, which is what the split above is protecting, never gets
/// here at all.
pub fn swap_stack(new: Vec<Frame>) -> Vec<Frame> {
    let previous = with_frames(|f| f.to_vec());
    STACK.with(|s| s.top.set(s.base.get()));
    for fr in new {
        push_frame(fr);
    }
    previous
}

/// The RAII half: construction pushes, `Drop` pops -- bind it to a `let`
/// at the top of a generated method body (`let __frame = ...;`) and every
/// exit path (tail value, early `return`, `?`) pops exactly once.
pub struct FrameGuard(());

impl FrameGuard {
    // `#[inline]` on push/drop: these run on EVERY method call from the
    // generated crate, which is a separate rustc invocation -- without the
    // hint (and an optimized generated build) each is a cross-crate call.
    #[inline]
    pub fn push(file: &'static str, method: &'static str, line: u32, end_line: u32) -> FrameGuard {
        // A Rust-side guard brackets no pool scope (RAII drops own its
        // temporaries); the pop skips the drain.
        push_frame(Frame::with_label(file, method, line, end_line, Frame::NO_MARK));
        #[cfg(feature = "ext-tracepoint")]
        if end_line != 0 && crate::ext::tracepoint::tracing() {
            crate::ext::tracepoint::fire_entry(file, method, line);
        }
        FrameGuard(())
    }
}

impl Drop for FrameGuard {
    #[inline]
    fn drop(&mut self) {
        // The tracing gate comes FIRST so the untraced path pops in place.
        // A guard frame's mark is NO_MARK, so discarding it drains nothing.
        #[cfg(feature = "ext-tracepoint")]
        if crate::ext::tracepoint::tracing() {
            traced_pop();
            return;
        }
        pop_frame_discard();
    }
}

/// The pop while tracing is on: `:return`/`:end` for an event-bearing
/// frame; answers the popped frame's `pool_mark` like
/// [`pop_frame_discard`]. `#[cold]`-outlined so `Drop`'s inlined fast
/// path stays small. The frame is read out BEFORE the handler runs, since
/// the handler runs Ruby code that pushes frames of its own.
#[cfg(feature = "ext-tracepoint")]
#[cold]
fn traced_pop() -> u32 {
    let popped = STACK.with(|s| {
        let top = s.top.get();
        if top == s.base.get() {
            return None;
        }
        let top = unsafe { top.sub(1) };
        s.top.set(top);
        Some(unsafe { *top })
    });
    let Some(fr) = popped else {
        return Frame::NO_MARK;
    };
    if fr.end_line != 0 {
        crate::ext::tracepoint::fire_exit(&fr);
    }
    fr.pool_mark
}

/// A frame for a C-implemented callee: CRuby shows such frames at the
/// CALLER's file:line (there is no Ruby-level line inside a C function),
/// so this clones the current innermost frame's location under `method`'s
/// label -- e.g. `'BasicObject#initialize'` when an `initialize`-less
/// `.new` rejects arguments. Same RAII contract as [`FrameGuard::push`].
///
/// An EXACT repeat of the innermost frame (same label, same location) is
/// skipped: one logical C call shows one frame, and a row with a
/// hand-placed frame inside it is also framed at the dispatch boundary
/// (`dispatch::with_c_frame`) -- without the dedupe every such call showed
/// twice in a backtrace.
pub fn synthetic_c_frame(method: &'static str) -> CFrameGuard {
    synthetic_c_frame_of(FrameMethod::Label(method))
}

/// [`synthetic_c_frame`] in the ids state: the cold dispatch boundary
/// pushes (owner, symbol, separator) verbatim and NO label materializes
/// unless something reads the frame -- see `Frame::method_ptr`'s docs.
pub(crate) fn synthetic_c_frame_ids(
    owner: crate::ClassId,
    sym: crate::Symbol,
    class_sep: bool,
) -> CFrameGuard {
    synthetic_c_frame_of(FrameMethod::Ids(owner, sym, class_sep))
}

fn synthetic_c_frame_of(method: FrameMethod) -> CFrameGuard {
    // ONE thread-local access for the whole sequence -- read the innermost
    // frame (caller location + dedupe), write, bump. The original spelling
    // (current_location + a dedupe scan + push_frame) paid four TLS
    // round-trips per builtin call, and this sits on every cached builtin
    // dispatch: measured as the bulk of a 2-4x regression on the
    // builtin-call-bound benchmarks (ruby_xor 0.996s -> 3.8s).
    let pushed = STACK.with(|s| {
        let top = s.top.get();
        let base = s.base.get();
        let (file, line) = if top != base && !top.is_null() {
            // SAFETY: `top > base`, so `top - 1` is the live innermost frame.
            let innermost = unsafe { &*top.sub(1) };
            // An EXACT repeat of the innermost frame: since the location
            // is CLONED from that same frame, "same label, same location"
            // reduces to a method match -- one logical C call shows one
            // frame. The boundary frame's method and a row's hand-placed
            // one can spell the same row from two sources (and now in two
            // STATES; `method_is` compares across them).
            if innermost.method_is(method) {
                return false;
            }
            (innermost.file, innermost.line)
        } else {
            ("", 0)
        };
        let fr = Frame::of(method, file, line, 0);
        if top == s.end.get() {
            // Full (or the null initial state): take the slow path outside.
            grow_and_push(fr);
            return true;
        }
        // SAFETY: `top < end`, a live slot.
        unsafe { top.write(fr) };
        s.top.set(unsafe { top.add(1) });
        true
    });
    CFrameGuard(pushed)
}

/// [`synthetic_c_frame`]'s guard: pops only what it pushed (a deduped call
/// pushed nothing). Delegates the pop to [`FrameGuard`]'s own Drop so the
/// traced-pop path stays in one place.
pub struct CFrameGuard(bool);

impl Drop for CFrameGuard {
    fn drop(&mut self) {
        if self.0 {
            drop(FrameGuard(()));
        }
    }
}

// A label handed over by a RUNTIME method install, for the one frame push
// that follows it.
//
// A `def` inside a runtime-minted class body (`Class.new { def m; end }`,
// `class D2 < SomethingRuntime`) is lifted to a top-level method and
// installed on the class afterwards, so the emitter bakes `Object#m` -- the
// only owner it can see. Ruby names the frame from the class's name AT RAISE
// TIME, which is not a compile-time fact at all: the class may be anonymous
// until a constant assignment names it.
//
// So the installer, which knows the real class, hands the label over and the
// next push takes it. ONE-SHOT on purpose: a top-level `def foo` called from
// inside such a method must keep its own `Object#foo`, and matching on the
// name alone would mislabel a top-level `def m` called from a runtime `m`.
thread_local! {
    static PENDING_LABEL: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Hand `label` to the next frame push on this thread. Answers the previous
/// value so the caller can restore it -- a body that pushes no frame of its
/// own must not leak the label to whatever pushes next.
///
/// Arming a label also latches `GATE_FRAMES_INDIRECT`: an emitted inline
/// prologue never consults the handover, so from here on prologues must
/// CALL their pushes.
pub fn set_pending_frame_label(label: Option<&'static str>) -> Option<&'static str> {
    if label.is_some() {
        crate::runtime_meta::arm_frames_indirect();
    }
    PENDING_LABEL.with(|c| c.replace(label))
}

/// A `&'static str` for a label built at run time, one leak per distinct
/// string and cached. A frame holds `&'static str` because the overwhelming
/// majority are `.rodata`; a runtime-minted class's name is the exception.
pub fn intern_label(label: &str) -> &'static str {
    static LABELS: std::sync::Mutex<Option<crate::FMap<String, &'static str>>> =
        std::sync::Mutex::new(None);
    let mut cache = LABELS.lock().expect("label cache");
    let map = cache.get_or_insert_with(crate::FMap::default);
    if let Some(&s) = map.get(label) {
        return s;
    }
    let leaked: &'static str = Box::leak(label.to_string().into_boxed_str());
    map.insert(label.to_string(), leaked);
    leaked
}

/// [`FrameGuard::push`] without the guard -- the capi push/pop twins call
/// these so Cranelift-compiled code (which has no Rust drops) brackets a
/// frame explicitly while the traced-pop logic stays in one place.
/// `pool_mark` is the release-pool watermark this frame's pop drains to
/// ([`Frame::NO_MARK`] = no pool scope).
pub(crate) fn frame_push_raw(
    file: &'static str,
    method: &'static str,
    line: u32,
    end_line: u32,
    pool_mark: u32,
) {
    // The handover replaces only the emitter's UNKNOWN-OWNER fallback. The
    // same install path also carries a genuine `define_method` block, whose
    // body keeps its own `block in ...` label in ruby -- overriding that one
    // renamed every such frame after its class. Taken either way, so the
    // one-shot still expires. The common path (no label armed) is one read,
    // no write.
    let method = match PENDING_LABEL.with(|c| c.get()) {
        None => method,
        Some(label) => {
            PENDING_LABEL.with(|c| c.set(None));
            if method.starts_with("Object#") {
                label
            } else {
                method
            }
        }
    };
    push_frame(Frame::with_label(file, method, line, end_line, pool_mark));
    #[cfg(feature = "ext-tracepoint")]
    if end_line != 0 && crate::ext::tracepoint::tracing() {
        crate::ext::tracepoint::fire_entry(file, method, line);
    }
}

/// The explicit pop matching [`frame_push_raw`]: answers the popped
/// frame's `pool_mark` so the capi pop can drain the release pool to it.
pub(crate) fn frame_pop_raw() -> u32 {
    #[cfg(feature = "ext-tracepoint")]
    if crate::ext::tracepoint::tracing() {
        return traced_pop();
    }
    pop_frame_discard()
}

/// [`synthetic_c_frame`] without the guard: answers whether a frame was
/// actually pushed (a deduped call pushes nothing) -- pass it back to
/// [`synthetic_c_frame_pop_raw`].
pub(crate) fn synthetic_c_frame_push_raw(method: &'static str) -> bool {
    let guard = synthetic_c_frame(method);
    let pushed = guard.0;
    std::mem::forget(guard);
    pushed
}

/// The explicit pop matching [`synthetic_c_frame_push_raw`].
pub(crate) fn synthetic_c_frame_pop_raw(pushed: bool) {
    drop(CFrameGuard(pushed));
}

/// Stamp the innermost frame's current line -- emitted before a statement
/// whose source line differs from the previous statement's.
#[inline]
pub fn set_line(line: u32) {
    STACK.with(|s| {
        let top = s.top.get();
        if top != s.base.get() {
            // SAFETY: `top > base`, so `top - 1` is the innermost frame.
            unsafe { (*top.sub(1)).line = line };
        }
    });
    #[cfg(feature = "ext-tracepoint")]
    if crate::ext::tracepoint::tracing() {
        crate::ext::tracepoint::fire_line(line);
    }
}

/// The innermost frame, copied out -- `TracePoint`'s `:raise` event derives
/// its path/lineno/method from the frame executing the raise.
#[cfg(feature = "ext-tracepoint")]
pub fn current_frame() -> Option<Frame> {
    with_frames(|f| f.last().copied())
}

/// How many frames are live -- the depth key for `tracepoint`'s armed-only
/// self notes.
#[cfg(feature = "ext-tracepoint")]
pub fn depth() -> usize {
    with_frames(<[Frame]>::len)
}

/// `Kernel#__method__` / `#__callee__`: the innermost frame's METHOD name,
/// with the owner qualifier the backtrace label carries stripped off.
/// `None` at the top level and inside a block, which is what ruby answers
/// there too. A builtin row pushes no frame of its own, so the top frame is
/// the caller whose name is being asked for.
///
/// BOTH separators, and the second one is not decoration. An instance method
/// is labelled `Owner#name` and a class or singleton method `Owner.name`, so
/// stripping only `#` left every `def self.x` answering `:"Owner.x"`. What
/// that breaks is `to_enum(__method__, ...)`, the standard way a method hands
/// back an enumerator over itself: the enumerator named a method that does
/// not exist, and re-entering it raised `NoMethodError`. rubygems' vendored
/// `TSort.tsort` is written exactly that way, so no gem could be resolved.
pub fn current_frame_method() -> Option<&'static str> {
    with_frames(|f| {
        // A block's label names the method it was written in
        // (`block (2 levels) in Object#m`), and that is the name ruby answers
        // from inside it -- so take everything after the last ` in `.
        let label = f.last()?.method();
        let owner = label.rsplit(" in ").next().unwrap_or(label);
        if owner.starts_with('<') {
            return None;
        }
        // The LAST separator: an owner may be `A::B`, and a method name can
        // hold neither character.
        Some(match owner.rsplit_once(['#', '.']) {
            Some((_, name)) => name,
            None => owner,
        })
    })
}

/// The innermost frame's `file:line`, for a caller-location label like
/// `Thread#inspect`'s creation site. A builtin C function has no frame of its
/// own, so the top frame is its caller.
pub fn current_location() -> Option<(&'static str, u32)> {
    with_frames(|f| f.last().map(|fr| (fr.file, fr.line)))
}

/// A path a frame can hold, from a String that is not one.
///
/// A `Frame` keeps `&'static str` so the push stays two stores, and every
/// compiled frame's file really is a program constant. Only `eval`'s own
/// `file` argument is not, and the set of distinct names one program evals
/// under is small and bounded -- interned rather than leaked per call, so
/// `eval(src, b, "f.rb", 1)` in a loop costs one allocation, not one a turn.
pub fn intern_path(path: &str) -> &'static str {
    use std::sync::{LazyLock, RwLock};
    static PATHS: LazyLock<RwLock<std::collections::HashSet<&'static str>>> =
        LazyLock::new(|| RwLock::new(std::collections::HashSet::new()));
    if let Some(p) = PATHS.read().unwrap().get(path) {
        return p;
    }
    let mut w = PATHS.write().unwrap();
    if let Some(p) = w.get(path) {
        return p;
    }
    let leaked: &'static str = Box::leak(path.to_string().into_boxed_str());
    w.insert(leaked);
    leaked
}

/// The innermost frame's LABEL verbatim (`Class#m`, `block in Class#m`,
/// `<main>`) -- what an `eval` under a Binding captured here reports as its
/// own frame, since CRuby runs the snippet in the captured scope's name.
pub fn current_frame_label() -> Option<&'static str> {
    with_frames(|f| Some(f.last()?.method()))
}

/// `FILE:LINE:in 'METHOD'` -- CRuby's backtrace-entry shape.
fn format_frame(fr: &Frame) -> String {
    format!("{}:{}:in '{}'", fr.file, fr.line, fr.method())
}

/// The current stack as formatted backtrace lines, INNERMOST FIRST --
/// what a raise stamps onto the exception (`Exception#backtrace`).
pub fn capture_backtrace() -> Vec<String> {
    with_frames(|f| f.iter().rev().map(format_frame).collect())
}

/// `Kernel#caller(start = 1)`: the formatted stack above the CALLING
/// frame. `start` counts frames to skip beyond the caller's own (CRuby's
/// contract; `caller` runs as a builtin with no frame of its own, so the
/// innermost frame IS the caller and `start = 1` skips exactly it).
pub fn caller_lines(start: usize) -> Vec<String> {
    with_frames(|f| f.iter().rev().skip(start).map(format_frame).collect())
}

/// `caller_lines`' structured twin, for `Kernel#caller_locations`: the same
/// frames as `(file, line, method)` rather than pre-formatted strings, so each
/// becomes a `Thread::Backtrace::Location` with real `#path`/`#lineno`/`#label`.
pub fn caller_frames(start: usize) -> Vec<(&'static str, u32, &'static str)> {
    with_frames(|f| {
        f.iter()
            .rev()
            .skip(start)
            .map(|fr| (fr.file, fr.line, fr.method()))
            .collect()
    })
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use zeo_abi::abi as a;

    /// The emitted-code contract behind `zeo_rt_frame_hot`: `FrameHot` and
    /// `Frame` offsets, including the `&str` (ptr, len) fat-pointer layout
    /// the two text fields assume. If a toolchain ever changes that
    /// layout, this fails loudly before any emitted store corrupts a
    /// frame.
    #[test]
    fn frame_hot_layout() {
        assert_eq!(std::mem::size_of::<Frame>(), a::FRAME_SIZE);
        assert_eq!(std::mem::offset_of!(FrameHot, top), a::FRAMEHOT_TOP);
        assert_eq!(std::mem::offset_of!(FrameHot, base), a::FRAMEHOT_BASE);
        assert_eq!(std::mem::offset_of!(FrameHot, end), a::FRAMEHOT_END);
        assert_eq!(std::mem::offset_of!(FrameHot, pool_top), a::FRAMEHOT_POOL_TOP);
        assert_eq!(std::mem::offset_of!(FrameHot, pool_base), a::FRAMEHOT_POOL_BASE);
        assert_eq!(std::mem::offset_of!(FrameHot, pool_end), a::FRAMEHOT_POOL_END);
        assert_eq!(std::mem::offset_of!(Frame, file), a::FRAME_FILE_PTR);
        assert_eq!(std::mem::offset_of!(Frame, method_ptr), a::FRAME_METHOD_PTR);
        assert_eq!(std::mem::offset_of!(Frame, method_len), a::FRAME_METHOD_LEN);
        assert_eq!(std::mem::offset_of!(Frame, line), a::FRAME_LINE);
        assert_eq!(std::mem::offset_of!(Frame, end_line), a::FRAME_END_LINE);
        assert_eq!(std::mem::offset_of!(Frame, pool_mark), a::FRAME_POOL_MARK);

        // The fat-pointer internals: write a frame the way an emitted
        // prologue does (raw stores at the ABI offsets), read it back
        // through the struct.
        let file = "file.rb";
        let method = "Object#m";
        let mut buf = [0u8; a::FRAME_SIZE];
        let p = buf.as_mut_ptr();
        unsafe {
            p.add(a::FRAME_FILE_PTR)
                .cast::<*const u8>()
                .write_unaligned(file.as_ptr());
            p.add(a::FRAME_FILE_LEN)
                .cast::<usize>()
                .write_unaligned(file.len());
            p.add(a::FRAME_METHOD_PTR)
                .cast::<*const u8>()
                .write_unaligned(method.as_ptr());
            p.add(a::FRAME_METHOD_LEN)
                .cast::<usize>()
                .write_unaligned(method.len());
            p.add(a::FRAME_LINE).cast::<u32>().write_unaligned(7);
            p.add(a::FRAME_END_LINE).cast::<u32>().write_unaligned(9);
            p.add(a::FRAME_POOL_MARK).cast::<u32>().write_unaligned(3);
            let fr: Frame = std::ptr::read_unaligned(p.cast());
            assert_eq!(fr.file, "file.rb");
            assert_eq!(fr.method(), "Object#m");
            assert_eq!(fr.line, 7);
            assert_eq!(fr.end_line, 9);
            assert_eq!(fr.pool_mark, 3);
        }
    }
}

#[cfg(test)]
mod pending_label_tests {
    use super::*;

    /// Push a frame the way an emitted body does, and read back the label it
    /// landed under.
    fn push_and_read(baked: &'static str) -> &'static str {
        frame_push_raw("t.rb", baked, 1, 0, Frame::NO_MARK);
        let got = current_frame_label().expect("a frame was pushed");
        let _ = frame_pop_raw();
        got
    }

    /// A handed-over label replaces the emitter's UNKNOWN-OWNER fallback.
    ///
    /// A `def` inside a runtime-minted class body is lifted to a top-level
    /// method, so the emitter can only bake `Object#name`. The installer knows
    /// the real class and hands the label over; ruby names such a frame from
    /// the class's name at RAISE time, which no compile-time table has.
    #[test]
    fn a_handed_over_label_replaces_the_object_fallback() {
        set_pending_frame_label(Some("K#m"));
        assert_eq!(push_and_read("Object#m"), "K#m");
    }

    /// It is ONE-SHOT. A top-level `def foo` called from inside such a method
    /// must keep its own label -- the pending is consumed by the first push,
    /// so the nested one cannot pick it up.
    #[test]
    fn a_handed_over_label_is_consumed_by_one_push() {
        set_pending_frame_label(Some("K#m"));
        assert_eq!(push_and_read("Object#m"), "K#m");
        assert_eq!(
            push_and_read("Object#helper"),
            "Object#helper",
            "the second push must not reuse the label"
        );
    }

    /// A label of any OTHER shape keeps its own. The same install path also
    /// carries a genuine `define_method` block, whose body reports
    /// `block in ...` in ruby -- overriding that renamed every one of them.
    /// Taken either way, so the one-shot still expires.
    #[test]
    fn a_block_label_is_not_replaced_and_still_expires() {
        set_pending_frame_label(Some("K#dm"));
        assert_eq!(
            push_and_read("block (2 levels) in <main>"),
            "block (2 levels) in <main>"
        );
        assert_eq!(
            push_and_read("Object#m"),
            "Object#m",
            "a rejected handover must still be consumed"
        );
    }

    /// With nothing pending, a push is exactly what the body baked -- the
    /// path every ordinary method takes.
    #[test]
    fn no_pending_label_leaves_the_baked_one() {
        set_pending_frame_label(None);
        assert_eq!(push_and_read("Plain#p1"), "Plain#p1");
    }

    /// The setter answers the PREVIOUS value, which is what lets a nested
    /// install restore its caller's pending rather than clearing it.
    #[test]
    fn setting_a_label_answers_the_previous_one() {
        set_pending_frame_label(None);
        assert_eq!(set_pending_frame_label(Some("A#x")), None);
        assert_eq!(set_pending_frame_label(Some("B#y")), Some("A#x"));
        assert_eq!(set_pending_frame_label(None), Some("B#y"));
    }

    /// One leak per distinct string, and the same string answers the same
    /// pointer -- a runtime-minted class's frames must not leak per call.
    #[test]
    fn an_interned_label_is_leaked_once() {
        let a = intern_label("Runtime#method");
        let b = intern_label("Runtime#method");
        assert_eq!(a, b);
        assert!(std::ptr::eq(a, b), "the cache must answer the same pointer");
    }
}
