//! Cells, C-bodied procs, `yield`, and `Proc#call` -- how a Cranelift
//! block body becomes (and is invoked as) a real escaping `RProc`.

use super::dispatch::status_out;
use crate::builtins::binding::LocalCell;
use crate::{RubyValue, Signal};
use std::sync::Arc;

/// The C cell handle: a raw `LocalCell` (`Arc<parking_lot::Mutex<
/// RubyValue>>`) pointer, opaque to compiled code, shuttled between
/// `zeo_rt_cell_*` and the `ProcEnv.cells` array.
pub type Cell = parking_lot::Mutex<RubyValue>;

/// A compiled block body. `env` carries the captured cells and the lexical
/// context; `self_` is the receiver this invocation runs under (a
/// parameter, never captured -- `instance_exec` rebinding); `blk` is the
/// CALL-SITE block, moved in (null = none); `out`/status as everywhere.
pub type BlockFn = unsafe extern "C" fn(
    env: *const ProcEnv,
    self_: *const RubyValue,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32;

/// The per-call view a [`BlockFn`] receives: raw pointers into the owned
/// environment the `RProc` closure holds. Null `lexical_blk`/`binding`
/// mean the enclosing scope had none.
#[repr(C)]
pub struct ProcEnv {
    pub cells: *const *mut Cell,
    pub n_cells: usize,
    pub lexical_blk: *const RubyValue,
    pub binding: *const RubyValue,
}

/// The raw cell-pointer array [`ProcEnv`] exposes, built once at proc
/// construction.
///
/// SAFETY of the `Send + Sync` claim: the pointers are non-owning views of
/// the `LocalCell`s held right beside them in [`ProcEnvOwned`] (which keep
/// the pointees alive), and the pointees are `Mutex`es -- `Sync` by
/// design. The raw array itself is never mutated after construction.
struct CellPtrs(Box<[*mut Cell]>);
unsafe impl Send for CellPtrs {}
unsafe impl Sync for CellPtrs {}

/// What a C-bodied proc OWNS: one reference per captured cell (kept so the
/// raw view stays live), the enclosing method's block for a nested bare
/// `yield`, and the enclosing `binding` value when one was captured.
pub struct ProcEnvOwned {
    cells: Box<[LocalCell]>,
    ptrs: CellPtrs,
    lexical_blk: Option<RubyValue>,
    binding: Option<RubyValue>,
}

impl ProcEnvOwned {
    /// The `RubyValue`s this environment owns: the enclosing method's block
    /// and the captured `binding`. The CELLS travel [`ProcEnvOwned::gc_cells`]
    /// instead, and a cell's own contents are enumerated by the cell.
    pub(crate) fn gc_edges(&self, out: &mut Vec<crate::RubyValue>) {
        out.extend(self.lexical_blk.iter().cloned());
        out.extend(self.binding.iter().cloned());
    }

    /// Every captured cell, by address -- the identity a registered cell node
    /// answers with. This is the edge that makes `obj.callback = -> { obj }`
    /// visible to the collector at all.
    pub(crate) fn gc_cells(&self, out: &mut Vec<usize>) {
        out.extend(
            self.cells
                .iter()
                .map(|c| Arc::as_ptr(c) as *const () as usize),
        );
    }

    /// A second owner of the SAME cells, for a `dup`/`clone`/`#lambda` copy
    /// that shares the one closure allocation. The raw view is rebuilt
    /// rather than copied, so it points at the cells this struct holds.
    pub(crate) fn share(&self) -> ProcEnvOwned {
        ProcEnvOwned::new(
            self.cells.clone(),
            self.lexical_blk.clone(),
            self.binding.clone(),
        )
    }

    fn new(
        cells: Box<[LocalCell]>,
        lexical_blk: Option<RubyValue>,
        binding: Option<RubyValue>,
    ) -> Self {
        let ptrs = CellPtrs(cells.iter().map(|c| Arc::as_ptr(c).cast_mut()).collect());
        ProcEnvOwned {
            cells,
            ptrs,
            lexical_blk,
            binding,
        }
    }
}

/// Invoke a C block body from Rust (the `ProcFn` closure `RProc::from_c`
/// wraps): build the per-call `ProcEnv` view, move the block in, translate
/// the status back.
pub(crate) fn call_block_fn(
    f: BlockFn,
    env: &ProcEnvOwned,
    self_val: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let view = ProcEnv {
        cells: env.ptrs.0.as_ptr(),
        n_cells: env.cells.len(),
        lexical_blk: env
            .lexical_blk
            .as_ref()
            .map_or(std::ptr::null(), |v| v as *const RubyValue),
        binding: env
            .binding
            .as_ref()
            .map_or(std::ptr::null(), |v| v as *const RubyValue),
    };
    let mut out = std::mem::MaybeUninit::<RubyValue>::uninit();
    let mut blk = std::mem::ManuallyDrop::new(block);
    let blk_ptr = match &mut *blk {
        Some(b) => {
            super::leakcheck::created(b);
            b as *mut RubyValue
        }
        None => std::ptr::null_mut(),
    };
    let status = unsafe {
        f(
            &view,
            self_val,
            args.as_ptr(),
            args.len(),
            blk_ptr,
            out.as_mut_ptr(),
        )
    };
    if status == zeo_abi::abi::STATUS_OK {
        let v = unsafe { out.assume_init() };
        super::leakcheck::consumed(&v);
        Ok(v)
    } else {
        Err(crate::signal::take_pending()
            .expect("a compiled block answered STATUS_SIGNAL with an empty pending slot"))
    }
}

/// `zeo_rt_proc_new`'s flag bits.
pub const PROC_LAMBDA: u32 = 1;
/// Capture the current method activation as the proc's return home.
pub const PROC_HOME: u32 = 2;

/// A fresh cell holding the value moved from `init` -- the storage a
/// captured local escapes into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cell_new(init: *mut RubyValue) -> *mut Cell {
    let v = if init.is_null() {
        RubyValue::Nil
    } else {
        super::leakcheck::consumed(unsafe { &*init });
        unsafe { std::ptr::read(init) }
    };
    Arc::into_raw(crate::builtins::binding::new_cell(v)).cast_mut()
}

/// One more owner of the cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cell_retain(cell: *mut Cell) {
    unsafe { Arc::increment_strong_count(cell.cast_const()) };
}

/// Release one owner (frees the cell at zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cell_release(cell: *mut Cell) {
    // Null is "this scope never created a cell". A cell slot is nulled in
    // the entry block and filled where the binding happens, and the two can
    // be different paths -- a fused loop's inline arm creates the cell, and
    // the guard's fallback arm reaches the same epilogue without it.
    if cell.is_null() {
        return;
    }
    drop(unsafe { Arc::from_raw(cell.cast_const()) });
}

/// The cell's current value, cloned out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cell_load(cell: *mut Cell, out: *mut RubyValue) {
    let v = unsafe { &*cell }.lock().clone();
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Store the value moved from `v` into the cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cell_store(cell: *mut Cell, v: *mut RubyValue) {
    super::leakcheck::consumed(unsafe { &*v });
    let value = unsafe { std::ptr::read(v) };
    let old = std::mem::replace(&mut *unsafe { &*cell }.lock(), value);
    // Dropped OUTSIDE the lock: releasing an object graph can reach other
    // cells, never this one -- but keeping the guard narrow costs nothing.
    drop(old);
}

/// The shared head of the two construction entries: the retained cells,
/// the borrowed views, the env, the `#binding` capture and the home.
/// Params/location/outer differ per entry and are added by the caller.
unsafe fn proc_builder_of(
    f: BlockFn,
    cells: *const *mut Cell,
    n_cells: usize,
    self_: *const RubyValue,
    lexical_blk: *const RubyValue,
    binding: *const RubyValue,
    arity: i32,
    flags: u32,
) -> crate::rproc::ProcBuilder {
    let owned: Box<[LocalCell]> = (0..n_cells)
        .map(|i| {
            let raw = unsafe { *cells.add(i) }.cast_const();
            unsafe { Arc::increment_strong_count(raw) };
            unsafe { Arc::from_raw(raw) }
        })
        .collect();
    let opt = |p: *const RubyValue| {
        if p.is_null() {
            None
        } else {
            Some(unsafe { (*p).clone() })
        }
    };
    let defining_scope = opt(binding);
    let env = ProcEnvOwned::new(owned, opt(lexical_blk), defining_scope.clone());
    let mut b = crate::rproc::ProcBuilder::from_c(
        f,
        env,
        unsafe { (*self_).clone() },
        arity,
        flags & PROC_LAMBDA != 0,
    );
    // Two different readers of the same value: the ENV copy serves a
    // `binding`/bare `yield` written inside the body, this one serves
    // `Proc#binding` asked from outside.
    if let Some(scope) = defining_scope {
        b = b.binding(scope);
    }
    if flags & PROC_HOME != 0 {
        b = b.home();
    }
    b
}

fn finish_proc(b: crate::rproc::ProcBuilder, out: *mut RubyValue) {
    let v = RubyValue::Proc(b.build());
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Build a real `Proc` around a compiled block body. `cells` (retained:
/// the proc takes its own reference to each), `self_` / `lexical_blk` /
/// `binding` are borrowed (cloned; the latter two may be null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_proc_new(
    f: BlockFn,
    cells: *const *mut Cell,
    n_cells: usize,
    self_: *const RubyValue,
    lexical_blk: *const RubyValue,
    binding: *const RubyValue,
    arity: i32,
    flags: u32,
    params: *const zeo_abi::abi::ParamC,
    n_params: usize,
    file: *const u8,
    file_len: usize,
    line: u32,
    outer: *const u8,
    outer_len: usize,
    out: *mut RubyValue,
) {
    let mut b = unsafe {
        proc_builder_of(f, cells, n_cells, self_, lexical_blk, binding, arity, flags)
    };
    if n_params > 0 {
        let rows = unsafe { std::slice::from_raw_parts(params, n_params) };
        b = b.params(rows.iter().map(param_meta_of).collect());
    }
    if file_len > 0 {
        b = b.location(unsafe { super::str_slice(file, file_len) }, line);
    }
    // The Ractor-isolation verdict rides ON THE VALUE: a dynamic proc's
    // creation site and its `Ractor.new` site only meet at run time.
    if outer_len > 0 {
        b = b.outer_capture(unsafe { super::static_str(outer, outer_len) });
    }
    finish_proc(b, out);
}

fn param_meta_of(p: &zeo_abi::abi::ParamC) -> crate::ProcParamMeta {
    crate::ProcParamMeta {
        kind: proc_param_kind(p.kind),
        name: (p.name.len > 0).then(|| unsafe { super::str_slice(p.name.ptr, p.name.len) }),
    }
}

/// One `Proc#parameters` table per `.rodata` shape, materialized on the
/// FIRST construction from that shape and leaked (the shape itself lives
/// for the process); every later construction borrows it, which is what
/// keeps the shaped entry allocation-free per proc.
static SHAPE_PARAMS: std::sync::Mutex<
    Option<crate::FMap<usize, &'static [crate::ProcParamMeta]>>,
> = std::sync::Mutex::new(None);

fn shape_param_rows(s: &zeo_abi::abi::ProcShapeC) -> &'static [crate::ProcParamMeta] {
    let key = s.params as usize;
    let mut g = SHAPE_PARAMS.lock().expect("shape-param cache poisoned");
    let map = g.get_or_insert_with(crate::FMap::default);
    if let Some(rows) = map.get(&key) {
        return rows;
    }
    let rows = unsafe { std::slice::from_raw_parts(s.params, s.n_params as usize) };
    let table: &'static [crate::ProcParamMeta] =
        Box::leak(rows.iter().map(param_meta_of).collect::<Vec<_>>().into_boxed_slice());
    map.insert(key, table);
    table
}

/// [`zeo_rt_proc_new`] with the compile-time constants read from ONE
/// `.rodata` `ProcShapeC` row (`zeo_proc_shapes`) instead of nine call
/// arguments and a stack-built row array per creation. The
/// `Proc#parameters` table is materialized once per shape
/// ([`SHAPE_PARAMS`]), so a construction allocates nothing for it.
///
/// # Safety
/// `shape` points at program data that lives for the process -- `.rodata`
/// on the AOT path, JIT/eval data memory (never unloaded) otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_proc_new_shaped(
    f: BlockFn,
    cells: *const *mut Cell,
    n_cells: usize,
    self_: *const RubyValue,
    lexical_blk: *const RubyValue,
    binding: *const RubyValue,
    shape: *const zeo_abi::abi::ProcShapeC,
    out: *mut RubyValue,
) {
    let s = unsafe { &*shape };
    let mut b = unsafe {
        proc_builder_of(
            f, cells, n_cells, self_, lexical_blk, binding, s.arity, s.flags,
        )
    };
    if s.n_params > 0 {
        b = b.params_static(shape_param_rows(s));
    }
    if s.file.len > 0 {
        b = b.location(unsafe { super::str_slice(s.file.ptr, s.file.len) }, s.line);
    }
    if s.outer.len > 0 {
        b = b.outer_capture(unsafe { super::static_str(s.outer.ptr, s.outer.len) });
    }
    finish_proc(b, out);
}

/// `&expr` at a call site: Ruby's `rb_block_arg_to_proc` -- a Proc passes
/// through, a Symbol becomes its proc, nil means "no block", anything
/// else duck-types through `to_proc` (`TypeError` otherwise). `out` gets
/// the Proc, or Nil for "no block".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_block_arg_to_proc(v: *const RubyValue, out: *mut RubyValue) -> i32 {
    match crate::rproc::block_arg_to_proc(unsafe { &*v }.clone()) {
        Ok(Some(p)) => {
            // A `&expr` is CRuby's PROC handler, not an iseq handler: the
            // block was named before it got here. `Kernel#lambda` refuses one.
            if let RubyValue::Proc(pr) = &p {
                pr.clear_literal_block();
            }
            super::leakcheck::created(&p);
            unsafe { out.write(p) };
            zeo_abi::abi::STATUS_OK
        }
        Ok(None) => {
            unsafe { out.write(RubyValue::Nil) };
            zeo_abi::abi::STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            zeo_abi::abi::STATUS_SIGNAL
        }
    }
}

/// `yield`: invoke the call-site block (`blk` is the method's borrowed
/// block slot; null raises the no-block `LocalJumpError`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_yield(
    blk: *const RubyValue,
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    if blk.is_null() {
        crate::signal::set_pending(crate::dispatch::raise_no_block_yield());
        return zeo_abi::abi::STATUS_SIGNAL;
    }
    let args = if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };
    let r = match unsafe { &*blk } {
        RubyValue::Proc(p) => p.call(args),
        other => Err(crate::builtins::type_error!(
            "wrong block value (given {})",
            other.inspect_string()
        )),
    };
    status_out(r, out)
}

/// `yield(*a)` / `yield(v, **h)`: the argument list is only known at run
/// time, so the caller hands the Array it built (plus the keyword Hash, or
/// null) and the block binds from its contents through its ordinary
/// parameter machinery.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_yield_args(
    blk: *const RubyValue,
    args: *const RubyValue,
    kw: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    if blk.is_null() {
        crate::signal::set_pending(crate::dispatch::raise_no_block_yield());
        return zeo_abi::abi::STATUS_SIGNAL;
    }
    let RubyValue::Array(a) = (unsafe { &*args }) else {
        panic!("a splatted yield's args must be an Array")
    };
    let mut full: Vec<RubyValue> = a.lock().iter().cloned().collect();
    // A `**h` the yield spelled as KEYWORDS contributes nothing when it is
    // empty at run time: the block sees one fewer argument, not a `{}`.
    if !kw.is_null() {
        let h = unsafe { &*kw };
        if crate::value::collections::hash_len(&h.as_hash_unchecked()) != 0 {
            full.push(h.clone());
        }
    }
    let r = match unsafe { &*blk } {
        RubyValue::Proc(p) => p.call(&full),
        other => Err(crate::builtins::type_error!(
            "wrong block value (given {})",
            other.inspect_string()
        )),
    };
    status_out(r, out)
}

/// `Proc#call` -- the proc's own lexical self, with an optional call-site
/// block forwarded to its `&param` (moved in; null = none).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_proc_call(
    p: *const RubyValue,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let args = if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };
    let block = if blk.is_null() {
        None
    } else {
        super::leakcheck::consumed(unsafe { &*blk });
        Some(unsafe { std::ptr::read(blk) })
    };
    let r = match unsafe { &*p } {
        RubyValue::Proc(rp) => rp.call_with_block(args, block),
        other => Err(crate::builtins::type_error!(
            "no implicit conversion of {} into Proc",
            crate::builtins::class_name_of(other)
        )),
    };
    status_out(r, out)
}

/// A [`zeo_abi::abi::ParamC`] kind as `Proc#parameters` spells it.
fn proc_param_kind(kind: u8) -> &'static str {
    use zeo_abi::abi;
    match kind {
        abi::PARAM_OPT => "opt",
        abi::PARAM_REST => "rest",
        abi::PARAM_KEYREQ => "keyreq",
        abi::PARAM_KEY => "key",
        abi::PARAM_KEYREST => "keyrest",
        abi::PARAM_BLOCK => "block",
        _ => "req",
    }
}

/// `Kernel#binding` -- the caller's own frame, captured. The names and their
/// cells come from the emitter (only a CELL local can be in one, which is
/// why a `binding` in a scope promotes its locals), the `file`/`line` are
/// the call's, and `cref` is `u32::MAX` where a top-level binding must not
/// claim `Object`.
///
/// The name pointers are `.rodata`, so the `'static` the binding stores is
/// real.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_binding_new(
    self_: *const RubyValue,
    names: *const zeo_abi::abi::Str,
    cells: *const *mut Cell,
    n: usize,
    file: *const u8,
    file_len: usize,
    line: u32,
    box_id: u32,
    cref: u32,
    out: *mut RubyValue,
) {
    let locals: Vec<(&'static str, LocalCell)> = (0..n)
        .map(|i| {
            let name = unsafe { &*names.add(i) };
            let raw = unsafe { *cells.add(i) }.cast_const();
            unsafe { Arc::increment_strong_count(raw) };
            (unsafe { super::static_str(name.ptr, name.len) }, unsafe {
                Arc::from_raw(raw)
            })
        })
        .collect();
    let v = crate::builtins::binding::binding_new(
        unsafe { (*self_).clone() },
        locals,
        unsafe { super::static_str(file, file_len) },
        line,
        box_id,
        cref,
    );
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `blk.call(..)` where `blk` is the scope's own `&block` parameter -- a
/// value that is a Proc or nil, and nothing else.
///
/// A Proc reaches [`crate::RProc::call`] DIRECTLY rather than Proc's
/// dispatch row, which is what keeps a `break` inside an iterator's block
/// a `Signal::Break` for the iterator to catch instead of the
/// `LocalJumpError` a proc-closure's break raises (`Enumerable#first`
/// driving a user `each` that forwards its block). It is the same fold the
/// rustc backend applies wherever it can type the receiver as a Proc.
///
/// Anything else -- `nil` when no block was given -- takes the ordinary
/// explicit send, so the miss is ruby's own `NoMethodError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_proc_call_or_send(
    recv: *const RubyValue,
    sym: u32,
    argv: *const RubyValue,
    argc: usize,
    blk: *mut RubyValue,
    caller: u32,
    out: *mut RubyValue,
) -> i32 {
    let args = if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };
    let block = if blk.is_null() {
        None
    } else {
        super::leakcheck::consumed(unsafe { &*blk });
        Some(unsafe { std::ptr::read(blk) })
    };
    let recv = unsafe { &*recv };
    let r = match recv {
        RubyValue::Proc(p) => p.call_with_block(args, block),
        other => crate::dispatch::send_value_explicit_in(
            0,
            other,
            crate::Symbol::from_u32(sym),
            args,
            block,
            caller,
        ),
    };
    status_out(r, out)
}
