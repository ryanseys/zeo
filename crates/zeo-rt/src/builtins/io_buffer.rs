//! `IO::Buffer` -- fixed-size byte storage with typed value access, slicing,
//! file mapping, and direct IO transfer (io_buffer.c's surface).
//!
//! The memory model: every buffer is a WINDOW (`offset`, `len`) over a
//! shared `Backing` -- heap bytes or a real `mmap` region. Slices clone the
//! `Arc<Backing>` with a narrower window, so writes through a slice are
//! visible to the parent and a parent's `resize` keeps its slices readable
//! (CRuby's observable behavior: a pre-resize slice still answers). `.map`
//! is a genuine `mmap` of the file, so a SHARED writable map writes through
//! to disk exactly like CRuby's.
//!
//! One honest divergence, documented here once: `.for(string)` COPIES the
//! string's bytes instead of aliasing them (zeo strings live behind an
//! `Arc<Mutex<StrBuf>>`, so their bytes are not stably addressable). The
//! block form copies back into the string at exit, which reproduces CRuby's
//! observable end state; what it cannot reproduce is a concurrent observer
//! seeing mid-block writes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use parking_lot::Mutex as PlMutex;

use crate::builtins::integer::int_value;
use crate::builtins::{arg_error, block_or_enum, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, IO_BUFFER_CLASS};
use zeo_macros::ruby_class;

// Flag bits, exposed as the class constants of the same names.
const EXTERNAL: u32 = 1;
const INTERNAL: u32 = 2;
const MAPPED: u32 = 4;
const SHARED: u32 = 8;
const LOCKED: u32 = 32;
const PRIVATE: u32 = 64;
const READONLY: u32 = 128;

fn page_size() -> i64 {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as i64 }
}

const DEFAULT_SIZE: usize = 65536;

/// The bytes a window looks into: heap storage or a real file mapping.
/// `Map`'s pointer is `munmap`ed on drop; it is only ever touched under the
/// owning mutex, which is what makes the raw pointer Send/Sync-safe here.
enum Mem {
    Heap(Vec<u8>),
    Map { ptr: usize, len: usize },
}

struct Backing {
    mem: PlMutex<Mem>,
}

unsafe impl Send for Backing {}
unsafe impl Sync for Backing {}

impl Drop for Backing {
    fn drop(&mut self) {
        if let Mem::Map { ptr, len } = &*self.mem.lock() {
            unsafe { libc::munmap(*ptr as *mut libc::c_void, *len) };
        }
    }
}

impl Backing {
    fn heap(bytes: Vec<u8>) -> Arc<Backing> {
        Arc::new(Backing {
            mem: PlMutex::new(Mem::Heap(bytes)),
        })
    }

    /// The address `#inspect` prints -- the storage base, like CRuby's.
    fn base_addr(&self) -> usize {
        match &*self.mem.lock() {
            Mem::Heap(v) => v.as_ptr() as usize,
            Mem::Map { ptr, .. } => *ptr,
        }
    }

    fn with<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        match &*self.mem.lock() {
            Mem::Heap(v) => f(v),
            Mem::Map { ptr, len } => {
                f(unsafe { std::slice::from_raw_parts(*ptr as *const u8, *len) })
            }
        }
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        match &mut *self.mem.lock() {
            Mem::Heap(v) => f(v),
            Mem::Map { ptr, len } => {
                f(unsafe { std::slice::from_raw_parts_mut(*ptr as *mut u8, *len) })
            }
        }
    }

    /// Grow the heap storage to at least `n` bytes. The storage NEVER
    /// shrinks (a shrinking `#resize` narrows the window only), so live
    /// slices' windows always stay in range.
    fn ensure_len(&self, n: usize) {
        if let Mem::Heap(v) = &mut *self.mem.lock()
            && v.len() < n
        {
            v.resize(n, 0);
        }
    }
}

struct BufState {
    backing: Option<Arc<Backing>>,
    offset: usize,
    len: usize,
    /// The EXTERNAL/INTERNAL/MAPPED/SHARED/LOCKED/PRIVATE/READONLY bits --
    /// stored verbatim from `new(size, flags)` (CRuby displays whatever was
    /// given), derived for every other constructor.
    flags: u32,
    /// Displayed as `SLICE`; set on `#slice` children and `.for` buffers.
    slice: bool,
    /// Displayed as `FILE`; set on `.map` buffers.
    file: bool,
    /// The `.for(str) { }` write-back target.
    source: Option<RubyValue>,
}

pub(crate) struct RBuffer {
    state: PlMutex<BufState>,
    frozen: AtomicBool,
}

impl RubyObject for RBuffer {
    fn class_id(&self) -> ClassId {
        IO_BUFFER_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        // `#freeze` never protects the bytes (CRuby: only READONLY does).
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        // A dup is always a WRITABLE, INTERNAL, heap copy of the window --
        // `IO::Buffer.for("x").dup` is the documented mutable-copy idiom.
        let st = self.state.lock();
        let bytes = window_bytes(&st);
        drop(st);
        let d = Arc::new(RBuffer {
            state: PlMutex::new(BufState {
                len: bytes.len(),
                backing: Some(Backing::heap(bytes)),
                offset: 0,
                flags: INTERNAL,
                slice: false,
                file: false,
                source: None,
            }),
            frozen: AtomicBool::new(false),
        });
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

fn buffer_value(st: BufState) -> RubyValue {
    RubyValue::Object(Arc::new(RBuffer {
        state: PlMutex::new(st),
        frozen: AtomicBool::new(false),
    }))
}

fn as_buffer(v: &RubyValue) -> Option<Arc<RBuffer>> {
    match v {
        RubyValue::Object(o) => o.clone().as_any_rc().downcast::<RBuffer>().ok(),
        _ => None,
    }
}

fn recv_buffer(recv: &RubyValue) -> Arc<RBuffer> {
    as_buffer(recv).expect("IO::Buffer table row dispatched on a non-Buffer receiver")
}

fn heap_state(size: usize, flags: u32) -> BufState {
    BufState {
        backing: Some(Backing::heap(vec![0; size])),
        offset: 0,
        len: size,
        flags,
        slice: false,
        file: false,
        source: None,
    }
}

fn null_state() -> BufState {
    BufState {
        backing: None,
        offset: 0,
        len: 0,
        flags: 0,
        slice: false,
        file: false,
        source: None,
    }
}

/// The default flag word for an anonymous `new(size)` -- CRuby maps at page
/// size and beyond, allocates internally below it.
fn default_flags(size: usize) -> u32 {
    if size >= page_size() as usize {
        MAPPED
    } else {
        INTERNAL
    }
}

fn window_bytes(st: &BufState) -> Vec<u8> {
    match &st.backing {
        Some(b) => b.with(|bytes| bytes[st.offset..st.offset + st.len].to_vec()),
        None => Vec::new(),
    }
}

fn with_window<R>(st: &BufState, f: impl FnOnce(&[u8]) -> R) -> R {
    match &st.backing {
        Some(b) => b.with(|bytes| f(&bytes[st.offset..st.offset + st.len])),
        None => f(&[]),
    }
}

fn with_window_mut<R>(st: &BufState, f: impl FnOnce(&mut [u8]) -> R) -> R {
    match &st.backing {
        Some(b) => b.with_mut(|bytes| f(&mut bytes[st.offset..st.offset + st.len])),
        None => f(&mut []),
    }
}

// ---------------------------------------------------------------- guards

fn locked_error(msg: &str) -> Signal {
    raise_error("IO::Buffer::LockedError", msg.to_string())
}

fn access_error() -> Signal {
    raise_error(
        "IO::Buffer::AccessError",
        "Buffer is not writable!".to_string(),
    )
}

fn writable_guard(st: &BufState) -> Result<(), Signal> {
    if st.flags & READONLY != 0 {
        return Err(access_error());
    }
    Ok(())
}

fn range_error() -> Signal {
    arg_error!("Specified offset+length is bigger than the buffer size!")
}

/// The `offset >= 0` and `offset + length <= size` pair most rows validate.
fn check_range(st: &BufState, offset: i64, length: i64) -> Result<(usize, usize), Signal> {
    if offset < 0 {
        return Err(arg_error!("Offset can't be negative!"));
    }
    if length < 0 {
        return Err(arg_error!("Length can't be negative!"));
    }
    let (offset, length) = (offset as usize, length as usize);
    if offset + length > st.len {
        return Err(range_error());
    }
    Ok((offset, length))
}

// ---------------------------------------------------------------- types

/// One `buffer_type` symbol: byte width plus how to decode/encode.
#[derive(Clone, Copy)]
struct BufType {
    size: usize,
    signed: bool,
    float: bool,
    big_endian: bool,
}

fn buf_type(v: &RubyValue) -> Result<BufType, Signal> {
    let RubyValue::Symbol(s) = v else {
        return Err(arg_error!("Invalid type name!"));
    };
    let name = s.name();
    let (size, signed, float, big_endian) = match name.as_str() {
        // The single-byte types have no endianness, hence no lowercase form.
        "U8" => (1, false, false, false),
        "S8" => (1, true, false, false),
        "u16" => (2, false, false, false),
        "U16" => (2, false, false, true),
        "s16" => (2, true, false, false),
        "S16" => (2, true, false, true),
        "u32" => (4, false, false, false),
        "U32" => (4, false, false, true),
        "s32" => (4, true, false, false),
        "S32" => (4, true, false, true),
        "u64" => (8, false, false, false),
        "U64" => (8, false, false, true),
        "s64" => (8, true, false, false),
        "S64" => (8, true, false, true),
        "u128" => (16, false, false, false),
        "U128" => (16, false, false, true),
        "s128" => (16, true, false, false),
        "S128" => (16, true, false, true),
        "f32" => (4, false, true, false),
        "F32" => (4, false, true, true),
        "f64" => (8, false, true, false),
        "F64" => (8, false, true, true),
        _ => return Err(arg_error!("Invalid type name!")),
    };
    Ok(BufType {
        size,
        signed,
        float,
        big_endian,
    })
}

fn type_bounds(st: &BufState, offset: i64, t: BufType) -> Result<usize, Signal> {
    if offset < 0 {
        return Err(arg_error!("Offset can't be negative!"));
    }
    let offset = offset as usize;
    if offset + t.size > st.len {
        return Err(arg_error!(
            "Type extends beyond end of buffer! (offset={} > size={})",
            offset,
            st.len
        ));
    }
    Ok(offset)
}

fn decode(bytes: &[u8], t: BufType) -> RubyValue {
    let mut raw = [0u8; 16];
    if t.big_endian {
        raw[16 - t.size..].copy_from_slice(bytes);
    } else {
        raw[..t.size].copy_from_slice(bytes);
    }
    if t.float {
        return RubyValue::Float(if t.size == 4 {
            let b: [u8; 4] = bytes.try_into().expect("4-byte window");
            (if t.big_endian {
                f32::from_be_bytes(b)
            } else {
                f32::from_le_bytes(b)
            }) as f64
        } else {
            let b: [u8; 8] = bytes.try_into().expect("8-byte window");
            if t.big_endian {
                f64::from_be_bytes(b)
            } else {
                f64::from_le_bytes(b)
            }
        });
    }
    let unsigned = if t.big_endian {
        u128::from_be_bytes(raw)
    } else {
        u128::from_le_bytes(raw)
    };
    if t.signed {
        // Sign-extend from the type's own width.
        let shift = 128 - (t.size as u32 * 8);
        let signed = ((unsigned << shift) as i128) >> shift;
        match i64::try_from(signed) {
            Ok(v) => RubyValue::Int(v),
            Err(_) => int_value(BigInt::from(signed)),
        }
    } else {
        let masked = if t.size == 16 {
            unsigned
        } else {
            unsigned & ((1u128 << (t.size * 8)) - 1)
        };
        match i64::try_from(masked) {
            Ok(v) => RubyValue::Int(v),
            Err(_) => int_value(BigInt::from(masked)),
        }
    }
}

fn encode(value: &RubyValue, t: BufType, out: &mut [u8]) -> Result<(), Signal> {
    if t.float {
        let f = match value {
            RubyValue::Float(f) => *f,
            RubyValue::Int(i) => *i as f64,
            other => {
                return Err(type_error!(
                    "wrong argument type {} (expected Float)",
                    crate::builtins::class_name_of(other)
                ));
            }
        };
        if t.size == 4 {
            let b = if t.big_endian {
                (f as f32).to_be_bytes()
            } else {
                (f as f32).to_le_bytes()
            };
            out.copy_from_slice(&b);
        } else {
            let b = if t.big_endian {
                f.to_be_bytes()
            } else {
                f.to_le_bytes()
            };
            out.copy_from_slice(&b);
        }
        return Ok(());
    }
    // Integers WRAP to the type's width (256 -> 0, -1 -> 255 for :U8).
    let wide: i128 = match value {
        RubyValue::Int(i) => *i as i128,
        RubyValue::BigInt(b) => b
            .to_i128()
            .or_else(|| b.to_u128().map(|u| u as i128))
            .unwrap_or(0),
        other => {
            return Err(type_error!(
                "wrong argument type {} (expected Integer)",
                crate::builtins::class_name_of(other)
            ));
        }
    };
    let raw = if t.big_endian {
        (wide as u128).to_be_bytes()
    } else {
        (wide as u128).to_le_bytes()
    };
    if t.big_endian {
        out.copy_from_slice(&raw[16 - t.size..]);
    } else {
        out.copy_from_slice(&raw[..t.size]);
    }
    Ok(())
}

// ---------------------------------------------------------------- display

/// The flag words `#inspect`/`#to_s` print, in io_buffer.c's order.
fn flag_words(st: &BufState) -> String {
    if st.backing.is_none() {
        return " NULL".to_string();
    }
    let mut out = String::new();
    for (bit, word) in [
        (EXTERNAL, "EXTERNAL"),
        (INTERNAL, "INTERNAL"),
        (MAPPED, "MAPPED"),
    ] {
        if st.flags & bit != 0 {
            out.push(' ');
            out.push_str(word);
        }
    }
    if st.file {
        out.push_str(" FILE");
    }
    for (bit, word) in [
        (SHARED, "SHARED"),
        (LOCKED, "LOCKED"),
        (PRIVATE, "PRIVATE"),
        (READONLY, "READONLY"),
    ] {
        if st.flags & bit != 0 {
            out.push(' ');
            out.push_str(word);
        }
    }
    if st.slice {
        out.push_str(" SLICE");
    }
    out
}

fn header(st: &BufState) -> String {
    let addr = st
        .backing
        .as_ref()
        .map(|b| b.base_addr() + st.offset)
        .unwrap_or(0);
    format!("#<IO::Buffer 0x{:016x}+{}{}>", addr, st.len, flag_words(st))
}

fn hexdump_lines(bytes: &[u8], base: usize, width: usize) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(width).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("0x{:08x}  ", base + i * width));
        for j in 0..width {
            match chunk.get(j) {
                Some(b) => out.push_str(&format!("{b:02x} ")),
                None => out.push_str("   "),
            }
        }
        for b in chunk {
            out.push(if (0x20..0x7f).contains(b) {
                *b as char
            } else {
                '.'
            });
        }
    }
    out
}

// ---------------------------------------------------------------- io glue

/// The raw fd behind an IO argument, refusing a closed stream the way every
/// io_buffer entry point does.
fn io_fd(io: &RubyValue) -> Result<libc::c_int, Signal> {
    if crate::builtins::io::io_is_closed(io) {
        return Err(raise_error("IOError", "closed stream".to_string()));
    }
    crate::builtins::io::raw_fd(io)
}

/// `read`/`write`/`pread`/`pwrite` share one shape: the syscall count is
/// `size - buffer_offset` (the `length` argument only participates in the
/// bounds validation -- io_buffer.c's observable behavior for files).
fn io_args(st: &BufState, args: &[RubyValue], skip: usize) -> Result<usize, Signal> {
    let length = match args.get(skip) {
        None | Some(RubyValue::Nil) => None,
        Some(v) => Some(crate::builtins::arg_int!(v)),
    };
    let offset = match args.get(skip + 1) {
        None => 0,
        Some(v) => crate::builtins::arg_int!(v),
    };
    let (offset, _) = check_range(st, offset, length.unwrap_or(0))?;
    Ok(offset)
}

ruby_class! {
    Buffer = zeo_abi::IO_BUFFER_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    const PAGE_SIZE = RubyValue::Int(page_size());
    const DEFAULT_SIZE = RubyValue::Int({
        std::env::var("RUBY_IO_BUFFER_DEFAULT_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_SIZE as i64)
    });
    const EXTERNAL = RubyValue::Int(EXTERNAL as i64);
    const INTERNAL = RubyValue::Int(INTERNAL as i64);
    const MAPPED = RubyValue::Int(MAPPED as i64);
    const SHARED = RubyValue::Int(SHARED as i64);
    const LOCKED = RubyValue::Int(LOCKED as i64);
    const PRIVATE = RubyValue::Int(PRIVATE as i64);
    const READONLY = RubyValue::Int(READONLY as i64);
    const LITTLE_ENDIAN = RubyValue::Int(4);
    const BIG_ENDIAN = RubyValue::Int(8);
    const HOST_ENDIAN = RubyValue::Int(if cfg!(target_endian = "big") { 8 } else { 4 });
    const NETWORK_ENDIAN = RubyValue::Int(8);

    // `IO::Buffer.for(string)` -- readonly view without a block, writable
    // with one (the block form copies back into the string at exit; see the
    // module doc for the aliasing divergence).
    def self."for"(_recv, arg, &block) {
        let RubyValue::Str(s) = arg else {
            return Err(type_error!(
                "wrong argument type {} (expected String)",
                crate::builtins::class_name_of(arg)
            ));
        };
        let bytes = s.lock().bytes().to_vec();
        let len = bytes.len();
        let mut st = BufState {
            backing: Some(Backing::heap(bytes)),
            offset: 0,
            len,
            flags: EXTERNAL | READONLY,
            slice: true,
            file: false,
            source: None,
        };
        let Some(block) = block else {
            return Ok(buffer_value(st));
        };
        st.flags = EXTERNAL;
        st.slice = false;
        st.source = Some(arg.clone());
        let buf = buffer_value(st);
        let RubyValue::Proc(p) = &block else {
            unreachable!("a literal block is a Proc");
        };
        let result = p.call(std::slice::from_ref(&buf));
        // Write back and detach, error or not (CRuby's ensure).
        let b = recv_buffer(&buf);
        let mut bst = b.state.lock();
        if let (Some(RubyValue::Str(dst)), Some(backing)) = (bst.source.take(), bst.backing.take())
        {
            let bytes = backing.with(|all| all[bst.offset..bst.offset + bst.len].to_vec());
            let mut d = dst.lock();
            let enc = d.encoding();
            d.replace_bytes(bytes, enc);
        }
        bst.len = 0;
        bst.offset = 0;
        drop(bst);
        result
    }

    // `IO::Buffer.map(file, size = nil, offset = 0, flags = SHARED)` -- a
    // REAL mmap: a shared writable map writes through to the file.
    def self."map"(_recv, *args) {
        crate::builtins::check_arity(args.len(), 1, Some(4))?;
        let fd = io_fd(&args[0])?;
        let offset = match args.get(2) {
            None => 0i64,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let flags = match args.get(3) {
            None => SHARED,
            Some(v) => crate::builtins::arg_int!(v) as u32,
        };
        let size = match args.get(1) {
            None | Some(RubyValue::Nil) => {
                let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
                if unsafe { libc::fstat(fd, &mut stat) } != 0 {
                    return Err(crate::builtins::file::raise_errno(
                        &std::io::Error::last_os_error(),
                        "io_buffer_map_file:fstat",
                        "",
                    ));
                }
                (stat.st_size - offset).max(0) as usize
            }
            Some(v) => crate::builtins::arg_int!(v) as usize,
        };
        let writable = flags & READONLY == 0;
        let prot = if writable {
            libc::PROT_READ | libc::PROT_WRITE
        } else {
            libc::PROT_READ
        };
        let map_kind = if flags & PRIVATE != 0 {
            libc::MAP_PRIVATE
        } else {
            libc::MAP_SHARED
        };
        let ptr = unsafe {
            libc::mmap(std::ptr::null_mut(), size, prot, map_kind, fd, offset as libc::off_t)
        };
        if ptr == libc::MAP_FAILED {
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(),
                "io_buffer_map_file:mmap",
                "",
            ));
        }
        let shared_bit = if flags & PRIVATE != 0 { 0 } else { SHARED };
        Ok(buffer_value(BufState {
            backing: Some(Arc::new(Backing {
                mem: PlMutex::new(Mem::Map { ptr: ptr as usize, len: size }),
            })),
            offset: 0,
            len: size,
            flags: EXTERNAL | MAPPED | shared_bit | (flags & (READONLY | PRIVATE)),
            slice: false,
            file: true,
            source: None,
        }))
    }

    def self."size_of"(_recv, arg) {
        match arg {
            RubyValue::Array(a) => {
                let items = a.lock().to_vec();
                let mut total = 0i64;
                for t in &items {
                    total += buf_type(t)?.size as i64;
                }
                Ok(RubyValue::Int(total))
            }
            other => Ok(RubyValue::Int(buf_type(other)?.size as i64)),
        }
    }

    // `IO::Buffer.string(n) { |buf| ... }` -- build a string through a
    // temporary buffer.
    def self."string"(_recv, arg, &block) {
        let n = crate::builtins::arg_int!(arg) as usize;
        let Some(RubyValue::Proc(p)) = block else {
            return Err(raise_error("LocalJumpError", "no block given".to_string()));
        };
        let buf = buffer_value(heap_state(n, INTERNAL));
        p.call(std::slice::from_ref(&buf))?;
        let b = recv_buffer(&buf);
        let st = b.state.lock();
        let bytes = window_bytes(&st);
        Ok(RubyValue::Str(crate::string_from_bytes(
            bytes,
            crate::encoding::ASCII_8BIT,
        )))
    }

    private def "initialize"(recv, *args) {
        init_in_place(recv, args)?;
        Ok(recv.clone())
    }

    // Becomes an internal heap copy of the source's window; answers the
    // copied byte count (io_buffer.c routes this through its copy).
    private def "initialize_copy"(recv, other) {
        let Some(src) = as_buffer(other) else {
            return Err(type_error!("initialize_copy should take same class object"));
        };
        let bytes = window_bytes(&src.state.lock());
        let n = bytes.len();
        let b = recv_buffer(recv);
        *b.state.lock() = BufState {
            backing: Some(Backing::heap(bytes)),
            offset: 0,
            len: n,
            flags: INTERNAL,
            slice: false,
            file: false,
            source: None,
        };
        Ok(RubyValue::Int(n as i64))
    }

    def "size"(recv) {
        Ok(RubyValue::Int(recv_buffer(recv).state.lock().len as i64))
    }
    def "valid?"(_recv) {
        // Nothing in this model dangles: slices hold their backing alive.
        Ok(RubyValue::Bool(true))
    }
    def "null?"(recv) {
        let b = recv_buffer(recv);
        let st = b.state.lock();
        Ok(RubyValue::Bool(st.backing.is_none() || st.len == 0))
    }
    def "empty?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().len == 0))
    }
    def "external?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().flags & EXTERNAL != 0))
    }
    def "internal?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().flags & INTERNAL != 0))
    }
    def "mapped?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().flags & MAPPED != 0))
    }
    def "shared?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().flags & SHARED != 0))
    }
    def "locked?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().flags & LOCKED != 0))
    }
    def "readonly?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().flags & READONLY != 0))
    }
    def "private?"(recv) {
        Ok(RubyValue::Bool(recv_buffer(recv).state.lock().flags & PRIVATE != 0))
    }

    def "locked"(recv, &block) {
        let b = recv_buffer(recv);
        {
            let mut st = b.state.lock();
            if st.flags & LOCKED != 0 {
                return Err(locked_error("Buffer already locked!"));
            }
            st.flags |= LOCKED;
        }
        let result = match &block {
            Some(RubyValue::Proc(p)) => p.call(std::slice::from_ref(recv)),
            _ => Err(raise_error("LocalJumpError", "no block given".to_string())),
        };
        b.state.lock().flags &= !LOCKED;
        result
    }

    def "free"(recv) {
        let b = recv_buffer(recv);
        let mut st = b.state.lock();
        if st.flags & LOCKED != 0 {
            return Err(locked_error("Buffer is locked!"));
        }
        *st = null_state();
        drop(st);
        Ok(recv.clone())
    }

    def "transfer"(recv) {
        let b = recv_buffer(recv);
        let mut st = b.state.lock();
        if st.flags & LOCKED != 0 {
            return Err(locked_error("Cannot transfer ownership of locked buffer!"));
        }
        let moved = std::mem::replace(&mut *st, null_state());
        drop(st);
        Ok(buffer_value(moved))
    }

    def "resize"(recv, arg) {
        let n = crate::builtins::arg_int!(arg);
        if n < 0 {
            return Err(arg_error!("Size can't be negative!"));
        }
        let n = n as usize;
        let b = recv_buffer(recv);
        let mut st = b.state.lock();
        if st.flags & LOCKED != 0 {
            return Err(locked_error("Cannot resize locked buffer!"));
        }
        if st.slice || st.file {
            return Err(raise_error(
                "IO::Buffer::AccessError",
                "Cannot resize external buffer!".to_string(),
            ));
        }
        match &st.backing {
            // Resize IN the existing backing so live slices keep answering.
            Some(back) => back.ensure_len(n),
            None => st.backing = Some(Backing::heap(vec![0; n])),
        }
        st.offset = 0;
        st.len = n;
        if st.flags == 0 {
            st.flags = default_flags(n);
        }
        drop(st);
        Ok(recv.clone())
    }

    def "slice"(recv, *args) {
        crate::builtins::check_arity(args.len(), 0, Some(2))?;
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let offset = match args.first() {
            None => 0,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let length = match args.get(1) {
            None => st.len as i64 - offset,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let (offset, length) = check_range(&st, offset, length)?;
        Ok(buffer_value(BufState {
            backing: st.backing.clone(),
            offset: st.offset + offset,
            len: length,
            flags: st.flags & READONLY,
            slice: true,
            file: false,
            source: None,
        }))
    }

    def "get_value"(recv, type_arg, offset_arg) {
        let t = buf_type(type_arg)?;
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let offset = type_bounds(&st, crate::builtins::arg_int!(offset_arg), t)?;
        Ok(with_window(&st, |w| decode(&w[offset..offset + t.size], t)))
    }

    def "set_value"(recv, type_arg, offset_arg, value) {
        let t = buf_type(type_arg)?;
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        let offset = type_bounds(&st, crate::builtins::arg_int!(offset_arg), t)?;
        with_window_mut(&st, |w| encode(value, t, &mut w[offset..offset + t.size]))?;
        // The offset AFTER the write (io_buffer.c's return), not the size.
        Ok(RubyValue::Int((offset + t.size) as i64))
    }

    def "get_values"(recv, types, offset_arg) {
        let RubyValue::Array(a) = types else {
            return Err(arg_error!("Argument buffer_types should be an array!"));
        };
        let items = a.lock().to_vec();
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let mut offset = crate::builtins::arg_int!(offset_arg);
        let mut out = Vec::with_capacity(items.len());
        for ty in &items {
            let t = buf_type(ty)?;
            let o = type_bounds(&st, offset, t)?;
            out.push(with_window(&st, |w| decode(&w[o..o + t.size], t)));
            offset += t.size as i64;
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }

    def "set_values"(recv, types, offset_arg, values) {
        let (RubyValue::Array(ts), RubyValue::Array(vs)) = (types, values) else {
            return Err(arg_error!("Argument buffer_types should be an array!"));
        };
        let (ts, vs) = (ts.lock().to_vec(), vs.lock().to_vec());
        if ts.len() != vs.len() {
            return Err(arg_error!(
                "Argument buffer_types and values should have the same length!"
            ));
        }
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        let mut offset = crate::builtins::arg_int!(offset_arg);
        for (ty, v) in ts.iter().zip(vs.iter()) {
            let t = buf_type(ty)?;
            let o = type_bounds(&st, offset, t)?;
            with_window_mut(&st, |w| encode(v, t, &mut w[o..o + t.size]))?;
            offset += t.size as i64;
        }
        // The offset AFTER the last write, like set_value.
        Ok(RubyValue::Int(offset))
    }

    def "get_string"(recv, *args) {
        crate::builtins::check_arity(args.len(), 0, Some(3))?;
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let offset = match args.first() {
            None => 0,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let length = match args.get(1) {
            None | Some(RubyValue::Nil) => st.len as i64 - offset,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let (offset, length) = check_range(&st, offset, length)?;
        let enc = match args.get(2) {
            None => crate::encoding::ASCII_8BIT,
            Some(e) => crate::builtins::encoding::arg_encoding(e)?,
        };
        let bytes = with_window(&st, |w| w[offset..offset + length].to_vec());
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, enc)))
    }

    def "set_string"(recv, *args) {
        crate::builtins::check_arity(args.len(), 1, Some(4))?;
        let RubyValue::Str(s) = &args[0] else {
            return Err(type_error!(
                "wrong argument type {} (expected String)",
                crate::builtins::class_name_of(&args[0])
            ));
        };
        let src = s.lock().bytes().to_vec();
        let offset = match args.get(1) {
            None => 0,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        let (offset, length) = check_range(&st, offset, src.len() as i64)?;
        with_window_mut(&st, |w| w[offset..offset + length].copy_from_slice(&src));
        Ok(RubyValue::Int((offset + length) as i64))
    }

    def "copy"(recv, *args) {
        crate::builtins::check_arity(args.len(), 1, Some(4))?;
        let Some(src) = as_buffer(&args[0]) else {
            return Err(type_error!(
                "wrong argument type {} (expected IO::Buffer)",
                crate::builtins::class_name_of(&args[0])
            ));
        };
        let src_bytes = window_bytes(&src.state.lock());
        let dest_off = match args.get(1) {
            None => 0,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let src_off = match args.get(3) {
            None => 0,
            Some(v) => crate::builtins::arg_int!(v),
        } as usize;
        if src_off > src_bytes.len() {
            return Err(range_error());
        }
        let length = match args.get(2) {
            None | Some(RubyValue::Nil) => (src_bytes.len() - src_off) as i64,
            Some(v) => crate::builtins::arg_int!(v),
        };
        if length as usize > src_bytes.len() - src_off {
            return Err(range_error());
        }
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        let (dest_off, length) = check_range(&st, dest_off, length)?;
        with_window_mut(&st, |w| {
            w[dest_off..dest_off + length].copy_from_slice(&src_bytes[src_off..src_off + length])
        });
        Ok(RubyValue::Int(length as i64))
    }

    def "clear"(recv, *args) {
        crate::builtins::check_arity(args.len(), 0, Some(3))?;
        let value = match args.first() {
            None => 0u8,
            Some(v) => crate::builtins::arg_int!(v) as u8,
        };
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        let offset = match args.get(1) {
            None => 0,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let length = match args.get(2) {
            None => st.len as i64 - offset,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let (offset, length) = check_range(&st, offset, length)?;
        with_window_mut(&st, |w| w[offset..offset + length].fill(value));
        drop(st);
        Ok(recv.clone())
    }

    def "<=>"(recv, other) {
        let Some(o) = as_buffer(other) else {
            return Err(type_error!(
                "wrong argument type {} (expected IO::Buffer)",
                crate::builtins::class_name_of(other)
            ));
        };
        let a = window_bytes(&recv_buffer(recv).state.lock());
        let bb = window_bytes(&o.state.lock());
        Ok(RubyValue::Int(match a.cmp(&bb) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }))
    }

    def "=="(recv, other) {
        let Some(o) = as_buffer(other) else {
            return Err(type_error!(
                "wrong argument type {} (expected IO::Buffer)",
                crate::builtins::class_name_of(other)
            ));
        };
        let a = window_bytes(&recv_buffer(recv).state.lock());
        let bb = window_bytes(&o.state.lock());
        Ok(RubyValue::Bool(a == bb))
    }

    // The non-mutating mask ops truncate to the SHORTER operand; the
    // in-place forms cycle the mask over the receiver. Both refuse an empty
    // mask.
    def "&"(recv, other) { mask_binop(recv, other, |a, b| a & b) }
    def "|"(recv, other) { mask_binop(recv, other, |a, b| a | b) }
    def "^"(recv, other) { mask_binop(recv, other, |a, b| a ^ b) }
    def "~"(recv) {
        let bytes: Vec<u8> = window_bytes(&recv_buffer(recv).state.lock())
            .iter()
            .map(|b| !b)
            .collect();
        let len = bytes.len();
        Ok(buffer_value(BufState {
            backing: Some(Backing::heap(bytes)),
            offset: 0,
            len,
            flags: INTERNAL,
            slice: false,
            file: false,
            source: None,
        }))
    }
    def "and!"(recv, other) { mask_inplace(recv, other, |a, b| a & b) }
    def "or!"(recv, other) { mask_inplace(recv, other, |a, b| a | b) }
    def "xor!"(recv, other) { mask_inplace(recv, other, |a, b| a ^ b) }
    def "not!"(recv) {
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        with_window_mut(&st, |w| w.iter_mut().for_each(|x| *x = !*x));
        drop(st);
        Ok(recv.clone())
    }

    def "each"(recv, *args, &block) {
        crate::builtins::check_arity(args.len(), 1, Some(3))?;
        let block = block_or_enum!(recv, args, block);
        let t = buf_type(&args[0])?;
        let b = recv_buffer(recv);
        let (mut offset, count) = each_bounds(&b, args, t)?;
        let mut seen = 0usize;
        loop {
            if count.is_some_and(|c| seen >= c) {
                break;
            }
            let st = b.state.lock();
            if offset + t.size > st.len {
                break;
            }
            let v = with_window(&st, |w| decode(&w[offset..offset + t.size], t));
            drop(st);
            block.call(&[RubyValue::Int(offset as i64), v])?;
            offset += t.size;
            seen += 1;
        }
        Ok(recv.clone())
    }

    def "values"(recv, *args) {
        crate::builtins::check_arity(args.len(), 1, Some(3))?;
        let t = buf_type(&args[0])?;
        let b = recv_buffer(recv);
        let (mut offset, count) = each_bounds(&b, args, t)?;
        let mut out = Vec::new();
        loop {
            if count.is_some_and(|c| out.len() >= c) {
                break;
            }
            let st = b.state.lock();
            if offset + t.size > st.len {
                break;
            }
            out.push(with_window(&st, |w| decode(&w[offset..offset + t.size], t)));
            offset += t.size;
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }

    def "each_byte"(recv, *_args, &block) {
        let block = block_or_enum!(recv, &[], block);
        let b = recv_buffer(recv);
        let mut i = 0usize;
        loop {
            let st = b.state.lock();
            if i >= st.len {
                break;
            }
            let byte = with_window(&st, |w| w[i]);
            drop(st);
            block.call(&[RubyValue::Int(byte as i64)])?;
            i += 1;
        }
        Ok(recv.clone())
    }

    def "hexdump"(recv, *args) {
        crate::builtins::check_arity(args.len(), 0, Some(3))?;
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let offset = match args.first() {
            None => 0,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let length = match args.get(1) {
            None => st.len as i64 - offset,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let width = match args.get(2) {
            None => 16usize,
            Some(v) => (crate::builtins::arg_int!(v)).max(1) as usize,
        };
        let (offset, length) = check_range(&st, offset, length)?;
        let bytes = with_window(&st, |w| w[offset..offset + length].to_vec());
        Ok(RubyValue::Str(crate::string_new(hexdump_lines(
            &bytes, offset, width,
        ))))
    }

    def "inspect"(recv) {
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let mut out = header(&st);
        if st.backing.is_some() && st.len > 0 {
            let shown = st.len.min(256);
            let bytes = with_window(&st, |w| w[..shown].to_vec());
            out.push('\n');
            out.push_str(&hexdump_lines(&bytes, 0, 16));
            if st.len > 256 {
                out.push_str(&format!("\n(and {} more bytes not printed)", st.len - 256));
            }
        }
        Ok(RubyValue::Str(crate::string_new(out)))
    }

    def "to_s"(recv) {
        Ok(RubyValue::Str(crate::string_new(header(
            &recv_buffer(recv).state.lock(),
        ))))
    }

    def "read"(recv, *args) {
        crate::builtins::check_arity(args.len(), 1, Some(3))?;
        let fd = io_fd(&args[0])?;
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        let offset = io_args(&st, args, 1)?;
        let n = with_window_mut(&st, |w| unsafe {
            libc::read(fd, w[offset..].as_mut_ptr() as *mut libc::c_void, w.len() - offset)
        });
        finish_io(n)
    }

    def "write"(recv, *args) {
        crate::builtins::check_arity(args.len(), 1, Some(3))?;
        let fd = io_fd(&args[0])?;
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let offset = io_args(&st, args, 1)?;
        let n = with_window(&st, |w| unsafe {
            libc::write(fd, w[offset..].as_ptr() as *const libc::c_void, w.len() - offset)
        });
        finish_io(n)
    }

    def "pread"(recv, *args) {
        crate::builtins::check_arity(args.len(), 2, Some(4))?;
        let fd = io_fd(&args[0])?;
        let from = crate::builtins::arg_int!(&args[1]);
        let b = recv_buffer(recv);
        let st = b.state.lock();
        writable_guard(&st)?;
        let offset = io_args(&st, args, 2)?;
        let n = with_window_mut(&st, |w| unsafe {
            libc::pread(
                fd,
                w[offset..].as_mut_ptr() as *mut libc::c_void,
                w.len() - offset,
                from as libc::off_t,
            )
        });
        finish_io(n)
    }

    def "pwrite"(recv, *args) {
        crate::builtins::check_arity(args.len(), 2, Some(4))?;
        let fd = io_fd(&args[0])?;
        let from = crate::builtins::arg_int!(&args[1]);
        let b = recv_buffer(recv);
        let st = b.state.lock();
        let offset = io_args(&st, args, 2)?;
        let n = with_window(&st, |w| unsafe {
            libc::pwrite(
                fd,
                w[offset..].as_ptr() as *const libc::c_void,
                w.len() - offset,
                from as libc::off_t,
            )
        });
        finish_io(n)
    }
}

/// `new`/`initialize` share one body: (re-)seat the receiver's state from
/// the `(size = DEFAULT_SIZE, flags = derived)` pair.
fn init_in_place(recv: &RubyValue, args: &[RubyValue]) -> Result<(), Signal> {
    crate::builtins::check_arity(args.len(), 0, Some(2))?;
    let size = match args.first() {
        None => DEFAULT_SIZE as i64,
        Some(v) => crate::builtins::arg_int!(v),
    };
    if size < 0 {
        return Err(arg_error!("Size can't be negative!"));
    }
    let flags = match args.get(1) {
        None => default_flags(size as usize),
        Some(v) => crate::builtins::arg_int!(v) as u32,
    };
    let b = recv_buffer(recv);
    *b.state.lock() = if size == 0 {
        null_state()
    } else {
        heap_state(size as usize, flags)
    };
    Ok(())
}

/// `each`/`values` share the (type, offset, count) argument shape.
fn each_bounds(
    b: &RBuffer,
    args: &[RubyValue],
    _t: BufType,
) -> Result<(usize, Option<usize>), Signal> {
    let st = b.state.lock();
    let offset = match args.get(1) {
        None => 0,
        Some(v) => crate::builtins::arg_int!(v),
    };
    let count = match args.get(2) {
        None => None,
        Some(v) => Some(crate::builtins::arg_int!(v) as usize),
    };
    let (offset, _) = check_range(&st, offset, 0)?;
    Ok((offset, count))
}

fn mask_guard(mask: &[u8]) -> Result<(), Signal> {
    if mask.is_empty() {
        return Err(raise_error(
            "IO::Buffer::MaskError",
            "Zero-length mask given!".to_string(),
        ));
    }
    Ok(())
}

fn mask_binop(
    recv: &RubyValue,
    other: &RubyValue,
    op: impl Fn(u8, u8) -> u8,
) -> Result<RubyValue, Signal> {
    let Some(o) = as_buffer(other) else {
        return Err(type_error!(
            "wrong argument type {} (expected IO::Buffer)",
            crate::builtins::class_name_of(other)
        ));
    };
    let a = window_bytes(&recv_buffer(recv).state.lock());
    let m = window_bytes(&o.state.lock());
    mask_guard(&m)?;
    // The mask CYCLES over the receiver (io_buffer.c) -- the result is
    // always receiver-sized.
    let bytes: Vec<u8> = a
        .iter()
        .enumerate()
        .map(|(i, x)| op(*x, m[i % m.len()]))
        .collect();
    let len = bytes.len();
    Ok(buffer_value(BufState {
        backing: Some(Backing::heap(bytes)),
        offset: 0,
        len,
        flags: INTERNAL,
        slice: false,
        file: false,
        source: None,
    }))
}

fn mask_inplace(
    recv: &RubyValue,
    other: &RubyValue,
    op: impl Fn(u8, u8) -> u8,
) -> Result<RubyValue, Signal> {
    let Some(o) = as_buffer(other) else {
        return Err(type_error!(
            "wrong argument type {} (expected IO::Buffer)",
            crate::builtins::class_name_of(other)
        ));
    };
    let m = window_bytes(&o.state.lock());
    mask_guard(&m)?;
    let b = recv_buffer(recv);
    let st = b.state.lock();
    writable_guard(&st)?;
    with_window_mut(&st, |w| {
        for (i, x) in w.iter_mut().enumerate() {
            *x = op(*x, m[i % m.len()]);
        }
    });
    drop(st);
    Ok(recv.clone())
}

fn finish_io(n: isize) -> Result<RubyValue, Signal> {
    if n < 0 {
        return Err(crate::builtins::file::raise_errno(
            &std::io::Error::last_os_error(),
            "io_buffer",
            "",
        ));
    }
    Ok(RubyValue::Int(n as i64))
}

fn buffer_allocate(_id: ClassId) -> RObj {
    Arc::new(RBuffer {
        state: PlMutex::new(null_state()),
        frozen: AtomicBool::new(false),
    })
}

/// `IO::Buffer.new` as a `ConstructorFn` rather than a table row, so
/// `singleton_methods(false)` stays `[:for, :map, :size_of, :string]` like
/// CRuby's (the Pathname precedent).
fn buffer_construct(
    _id: ClassId,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let buf = buffer_value(null_state());
    init_in_place(&buf, args)?;
    Ok(buf)
}

pub fn register_io_buffer(registry: &mut crate::dispatch::ClassRegistry) {
    registry.define_allocator(IO_BUFFER_CLASS, buffer_allocate);
    registry.register(
        IO_BUFFER_CLASS,
        "IO::Buffer",
        false,
        zeo_abi::declared_ancestors(IO_BUFFER_CLASS),
        Some(buffer_construct as crate::dispatch::ConstructorFn),
    );
}
