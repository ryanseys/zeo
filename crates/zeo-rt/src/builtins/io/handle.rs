//! The handle itself: what an `IO` object holds, how it reads its own
//! descriptor back, and the questions every other file here asks of it.

use super::*;

#[derive(Clone, Copy, PartialEq)]
pub enum StdStream {
    Stdout,
    Stderr,
    Stdin,
}

/// What an IO actually reads/writes. The std streams are process-global
/// handles (nothing to own); a File owns its open descriptor.
pub enum IoBackend {
    Std(StdStream),
    /// `IO.allocate` -- a handle that has never been opened. Distinct from a
    /// CLOSED one: ruby says `uninitialized stream` here and `closed stream`
    /// there, and names an uninitialized handle by address rather than by the
    /// path it has not got.
    Uninit,
    /// An open file. `None` after `close` -- a closed IO is not a dangling
    /// one: every operation on it raises IOError, which is what real Ruby
    /// does and what a plain `Option::take` gives us for free.
    File(Option<std::fs::File>),
    /// One end of an `IO.pipe`. Backed by a real fd (wrapped in a `File` for
    /// its `Read`/`Write`), but reports `IO` rather than `File` for `#class`
    /// and has no path -- a pipe is a plain IO, not a File.
    Pipe(Option<std::fs::File>),
}

impl IoBackend {
    /// Close the underlying descriptor (a no-op for the std streams), leaving a
    /// closed handle -- what `#close`/`#close_read`/`#close_write` need.
    ///
    /// The descriptor leaves through `close(2)` rather than through dropping
    /// the `File`, because the same fd may be under a SECOND IO (`IO.new(
    /// f.fileno)`, which autocloses too). Ruby's second close simply fails
    /// EBADF; Rust's `File` drop ABORTS the process on a failed close
    /// ("IO Safety violation: owned file descriptor already closed"), which
    /// killed the program at exit after it had run to completion. The result
    /// is handed back so `#close` can raise where ruby raises and teardown can
    /// stay quiet, which is the difference ruby itself draws.
    pub(super) fn close_file(&mut self) -> std::io::Result<()> {
        use std::os::fd::IntoRawFd;
        match self {
            IoBackend::File(slot) | IoBackend::Pipe(slot) => {
                let Some(f) = slot.take() else {
                    return Ok(());
                };
                // SAFETY: the fd was owned by the `File` just taken apart, and
                // `into_raw_fd` gives up that ownership -- so this is the one
                // and only close of it from here.
                match unsafe { libc::close(f.into_raw_fd()) } {
                    0 => Ok(()),
                    _ => Err(std::io::Error::last_os_error()),
                }
            }
            IoBackend::Std(_) | IoBackend::Uninit => Ok(()),
        }
    }

    /// Give the descriptor up WITHOUT closing it -- what `autoclose = false`
    /// promises, since the number belongs to whoever handed it over.
    pub(super) fn release_file(&mut self) {
        use std::os::fd::IntoRawFd;
        match self {
            IoBackend::File(slot) | IoBackend::Pipe(slot) => {
                if let Some(f) = slot.take() {
                    // Deliberately unclosed: the descriptor outlives us.
                    let _ = f.into_raw_fd();
                }
            }
            IoBackend::Std(_) | IoBackend::Uninit => {}
        }
    }
}

impl Drop for RIo {
    fn drop(&mut self) {
        let mut backend = self.backend.lock();
        if self.autoclose.load(std::sync::atomic::Ordering::Relaxed) {
            // Ruby closes at teardown and says nothing about a descriptor a
            // second IO already closed -- see `close_file`, which is also why
            // the `File` must not be left to drop itself.
            let _ = backend.close_file();
        } else {
            backend.release_file();
        }
    }
}

pub struct RIo {
    /// `Mutex` because reading MOVES the file position -- an IO is mutable
    /// shared state even behind an `Arc`, unlike the stateless std-stream
    /// singletons this started as.
    pub(super) backend: parking_lot::Mutex<IoBackend>,
    /// The path this was opened from, for error messages and `#path`. `None`
    /// when there is no associated file (a pipe, a std stream, or a
    /// `File.new(fd)` given no `path:`) -- distinct from `Some("")`, which
    /// `File.new(fd, path: "")` produces and `#path` must answer as `""`.
    /// Behind a `Mutex` because `#reopen` REPLACES it: the receiver keeps its
    /// descriptor number and takes on the target file.
    pub(super) path: parking_lot::Mutex<Option<String>>,
    /// The MODE string this handle was opened with, when a caller named one.
    /// `#reopen(path)` with no mode of its own inherits it, which is CRuby's
    /// rule (`rb_io_reopen` reuses `fptr->mode`) and the difference between
    /// `reopen`-ing a write handle and opening the target read-only.
    pub(super) open_mode: parking_lot::Mutex<Option<String>>,
    /// `#lineno` -- the count of lines read via `gets`/`readline`/`each_line`,
    /// which CRuby tracks per-IO and lets a program set with `lineno=`.
    pub(super) lineno: std::sync::atomic::AtomicI64,
    /// `#binmode?` -- on Unix binary vs text mode has no behavioural effect
    /// (no CRLF translation), so this only records what `#binmode` was told,
    /// exactly what `#binmode?` reports.
    pub(super) binmode: std::sync::atomic::AtomicBool,
    /// `#autoclose?` -- whether closing this IO closes its fd. Defaults to
    /// true; a program may clear it (`autoclose = false`) to keep the fd open.
    pub(super) autoclose: std::sync::atomic::AtomicBool,
    /// `#sync` -- whether a write reaches the descriptor at once. Every write
    /// here already does, so this only RECORDS what CRuby would report: true
    /// for STDERR, a socket, a popen handle, and a pipe's write end; false for
    /// a file, STDOUT, STDIN, and a pipe's read end.
    pub(super) sync: std::sync::atomic::AtomicBool,
    /// `Kernel#freeze`'s own flag. It gates nothing -- what a handle holds
    /// is a descriptor, not a Ruby-visible field -- but `frozen?` answers
    /// what was written, which a hardcoded `false` did not.
    pub(super) frozen: std::sync::atomic::AtomicBool,
    /// Bytes pushed back by `#ungetbyte`/`#ungetc`, read out (LIFO) before the
    /// stream itself. The next byte read drains this first.
    pub(super) unget: parking_lot::Mutex<Vec<u8>>,
    /// Bytes read AHEAD of what Ruby has consumed -- see [`ReadBuf`]. Only the
    /// line/char/byte readers fill it; `with_file` gives every other row a
    /// descriptor positioned exactly where Ruby thinks it is.
    pub(super) rbuf: parking_lot::Mutex<ReadBuf>,
    /// A socket reports `TCPSocket`/`TCPServer` (not `IO`) for `#class`, while
    /// still using a `Pipe`-shaped fd for read/write, and a handle built by a
    /// SUBCLASS receiver (`Class.new(File).open`) reports that subclass. `0`
    /// for ordinary files, pipes, and std streams, whose class comes from the
    /// backend. Atomic because the receiver's class is known one step after
    /// the handle is built, once it is already behind an `Arc`.
    pub(super) class_override: std::sync::atomic::AtomicU32,
    /// The child this IO is connected to, for an `IO.popen` handle (0 = none).
    /// `#pid` answers it, and `#close` reaps the child and sets `$?`.
    pub(super) child_pid: std::sync::atomic::AtomicI64,
    /// `#external_encoding`/`#internal_encoding` once `set_encoding` has been
    /// told. Unset, a READABLE stream reports `Encoding.default_external` and
    /// a write-only one reports nil -- CRuby's rule, and why this cannot just
    /// default to the process encoding.
    pub(super) encodings: parking_lot::Mutex<(
        Option<crate::encoding::EncodingId>,
        Option<crate::encoding::EncodingId>,
    )>,
    /// `#timeout`/`#timeout=` -- recorded and read back, nil by default. zeo's
    /// reads block, so nothing enforces it; see COMPATIBILITY.md.
    pub(super) timeout: parking_lot::Mutex<RubyValue>,
}

/// The byte-order mark a stream opens with, if any: the encoding it names and
/// how many bytes it occupies. UTF-32's marks are checked before UTF-16's,
/// since `FF FE 00 00` starts with UTF-16LE's own mark.
pub(super) fn read_bom(
    recv: &RubyValue,
) -> Result<Option<(crate::encoding::EncodingId, usize)>, Signal> {
    let head = with_file(recv, |f, path| {
        use std::io::{Read, Seek};
        let at = f
            .stream_position()
            .map_err(|e| crate::builtins::file::raise_errno(&e, "seek", path))?;
        let mut buf = [0u8; 4];
        let got = f
            .read(&mut buf)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "read", path))?;
        f.seek(std::io::SeekFrom::Start(at))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "seek", path))?;
        Ok(buf[..got].to_vec())
    })?;
    let enc = crate::encoding::self_describing_bom(&head);
    Ok(enc)
}

/// The read-ahead buffer behind `gets`/`each_line`/`getc`/`getbyte`.
///
/// Reading a line one `read(2)` at a time costs a syscall PER BYTE: an
/// unbuffered `bm_io_wordcount` spends 86% of its wall clock inside `read`,
/// against a CRuby that refills a buffer in chunks. Buffering here is the
/// same trade, with one rule that keeps it invisible: the descriptor sits
/// AHEAD of the position Ruby believes in by exactly `data.len() - pos`
/// bytes, and [`with_file`] seeks that difference back before handing the
/// descriptor to anything else. So `#read`, `#seek`, `#pos`, `#eof?`,
/// `#sysread` and every other row see precisely the unbuffered file, and
/// the buffer can only ever be filled where it can also be given back.
#[derive(Default)]
pub(super) struct ReadBuf {
    pub(super) data: Vec<u8>,
    /// How much of `data` the caller has already consumed.
    pub(super) pos: usize,
    /// Whether the descriptor can seek, so the unconsumed tail can be
    /// returned. `None` until probed once; a pipe, socket or std stream
    /// answers `false` and never buffers at all.
    pub(super) seekable: Option<bool>,
    /// How many of the unconsumed bytes were PUSHED BACK by `#ungetbyte`
    /// rather than read from the descriptor.
    ///
    /// They sit at the front of `data[pos..]` and the descriptor was never
    /// advanced past them, so [`unread`] must not seek back over them -- and
    /// must keep them, since nothing else holds a copy.
    pub(super) unget: usize,
}

impl ReadBuf {
    /// Bytes read but not yet handed out -- how far the descriptor sits ahead.
    pub(super) fn pending(&self) -> usize {
        self.data.len() - self.pos
    }

    /// `n` bytes have just been served out of `data[pos..]`; retire that many
    /// pushed-back bytes first, since they are the ones at the front.
    pub(super) fn consumed(&mut self, n: usize) {
        self.unget = self.unget.saturating_sub(n);
    }
}

impl RubyObject for RIo {
    // A File instance carries FILE_CLASS so its own MRO (`File < IO`) finds
    // File's rows before IO's; the std streams are plain IOs.
    fn class_id(&self) -> ClassId {
        match self
            .class_override
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => {}
            c => return ClassId(c),
        }
        match &*self.backend.lock() {
            IoBackend::File(_) => zeo_abi::FILE_CLASS,
            IoBackend::Std(_) | IoBackend::Pipe(_) | IoBackend::Uninit => IO_CLASS,
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // Freezing an IO gates nothing -- what a handle holds is a descriptor,
    // not a Ruby-visible field -- but `frozen?` answers what was written,
    // the std streams included.
    fn is_frozen(&self) -> bool {
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    // An IO's identity IS its descriptor -- `dup`ing one would have to `dup(2)`
    // the fd to mean anything. Answers a fresh handle on the same std stream;
    // a File `dup` is out of scope (see the module docs).
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        let stream = match &*self.backend.lock() {
            IoBackend::Std(s) => *s,
            IoBackend::File(_) | IoBackend::Pipe(_) | IoBackend::Uninit => StdStream::Stdout,
        };
        Arc::new(RIo::new(IoBackend::Std(stream), self.path.lock().clone()))
    }
}

impl RIo {
    /// This handle's live descriptor, or `None` once it is closed. A std
    /// stream's is its well-known number.
    pub(super) fn raw_fd(&self) -> Option<libc::c_int> {
        use std::os::fd::AsRawFd;
        match &*self.backend.lock() {
            IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => Some(f.as_raw_fd()),
            IoBackend::File(None) | IoBackend::Pipe(None) | IoBackend::Uninit => None,
            IoBackend::Std(StdStream::Stdin) => Some(0),
            IoBackend::Std(StdStream::Stdout) => Some(1),
            IoBackend::Std(StdStream::Stderr) => Some(2),
        }
    }

    /// `rb_io_reopen`'s core: point THIS handle's descriptor at whatever `src`
    /// refers to, keeping the descriptor NUMBER. That number is the reason
    /// reopen exists -- `$stderr.reopen(path)` has to redirect fd 2 itself, so
    /// a child process and every C-level write follow it.
    ///
    /// A std stream keeps its `Std` backend (its writes go through fd 1/2,
    /// which now points elsewhere); a file handle takes a fresh `File` over
    /// the same number, so it reads and writes the new target.
    pub(super) fn dup2_from(&self, src: libc::c_int, path: Option<String>) -> Result<(), Signal> {
        use std::os::fd::FromRawFd;
        let Some(dst) = self.raw_fd() else {
            return Err(crate::builtins::io_error!("closed stream"));
        };
        if src != dst {
            // SAFETY: both are live descriptors this process owns; `dup2`
            // closes `dst` first, which is exactly reopen's contract.
            if unsafe { libc::dup2(src, dst) } < 0 {
                return Err(crate::builtins::file::raise_errno(
                    &std::io::Error::last_os_error(),
                    "reopen",
                    path.as_deref().unwrap_or(""),
                ));
            }
        }
        let mut backend = self.backend.lock();
        if let IoBackend::File(slot) | IoBackend::Pipe(slot) = &mut *backend {
            // The old `File` owned `dst`, which `dup2` already closed and
            // reused. Forget it rather than dropping it, or Rust's IO-safety
            // check aborts on the double close, then re-own the same number.
            if let Some(old) = slot.take() {
                std::mem::forget(old);
            }
            // SAFETY: `dst` is open and now refers to the target.
            *slot = Some(unsafe { std::fs::File::from_raw_fd(dst) });
        }
        drop(backend);
        *self.path.lock() = path;
        self.unget.lock().clear();
        // Read-ahead from the old descriptor describes a file this IO no
        // longer refers to.
        *self.rbuf.lock() = ReadBuf::default();
        Ok(())
    }

    /// The one place RIo's default per-handle state (unset binmode, autoclose
    /// on, no pushed-back bytes, lineno 0) is established, so every constructor
    /// agrees.
    pub(super) fn new(backend: IoBackend, path: Option<String>) -> RIo {
        let sync = matches!(backend, IoBackend::Std(StdStream::Stderr));
        RIo {
            sync: std::sync::atomic::AtomicBool::new(sync),
            backend: parking_lot::Mutex::new(backend),
            path: parking_lot::Mutex::new(path),
            open_mode: parking_lot::Mutex::new(None),
            lineno: std::sync::atomic::AtomicI64::new(0),
            binmode: std::sync::atomic::AtomicBool::new(false),
            autoclose: std::sync::atomic::AtomicBool::new(true),
            frozen: std::sync::atomic::AtomicBool::new(false),
            unget: parking_lot::Mutex::new(Vec::new()),
            rbuf: parking_lot::Mutex::new(ReadBuf::default()),
            class_override: std::sync::atomic::AtomicU32::new(0),
            child_pid: std::sync::atomic::AtomicI64::new(0),
            encodings: parking_lot::Mutex::new((None, None)),
            timeout: parking_lot::Mutex::new(RubyValue::Nil),
        }
    }
}

/// Zeroes the per-handle state back to `RIo::new`'s defaults after a re-init
/// swaps the descriptor: lineno, binmode, autoclose, sync, pushed-back bytes,
/// read-ahead, child pid, encodings, timeout.
/// Records an opened handle's external/internal encodings, which is what a
/// read tags its bytes with. Separate from `set_encoding` the ruby method so
/// `File.open` can set them without going through dispatch.
pub(crate) fn set_handle_encodings(
    io: &RubyValue,
    ext: Option<crate::encoding::EncodingId>,
    int: Option<crate::encoding::EncodingId>,
) {
    if let Some(io) = as_rio(io) {
        *io.encodings.lock() = (ext, int);
    }
}

pub(super) fn reset_handle_state(io: &RIo) {
    use std::sync::atomic::Ordering;
    io.lineno.store(0, Ordering::Relaxed);
    io.binmode.store(false, Ordering::Relaxed);
    io.autoclose.store(true, Ordering::Relaxed);
    io.sync.store(false, Ordering::Relaxed);
    io.unget.lock().clear();
    *io.rbuf.lock() = ReadBuf::default();
    io.child_pid.store(0, Ordering::Relaxed);
    *io.encodings.lock() = (None, None);
    *io.timeout.lock() = RubyValue::Nil;
}

/// Wrap a connected socket fd (from a `TcpStream`/`TcpListener::accept`) as a
/// value that reports `class_id` for `#class` but reads/writes like a `Pipe`.
/// Shared by `TCPSocket.new` and `TCPServer#accept` (see `builtins::socket`).
pub(crate) fn socket_value(f: std::fs::File, class_id: ClassId) -> RubyValue {
    let mut io = RIo::new(IoBackend::Pipe(Some(f)), None);
    io.class_override
        .store(class_id.0, std::sync::atomic::Ordering::Relaxed);
    // A socket is unbuffered in CRuby, so it reports `sync` true.
    io.sync = std::sync::atomic::AtomicBool::new(true);
    RubyValue::Object(Arc::new(io))
}

/// Wrap an owning raw socket descriptor (from `libc::socket`/`accept`/
/// `socketpair`) as a Ruby socket value of `class_id`, reading/writing over the
/// fd like a pipe. The fd's ownership transfers to the value (closed on GC or
/// `#close`).
///
/// # Safety
/// `fd` must be a valid, open descriptor that nothing else owns.
pub(crate) unsafe fn socket_from_raw_fd(fd: std::os::fd::RawFd, class_id: ClassId) -> RubyValue {
    use std::os::fd::FromRawFd;
    socket_value(unsafe { std::fs::File::from_raw_fd(fd) }, class_id)
}

/// A handle that was never opened, tagged as `class_id` -- what `allocate`
/// answers for `IO`, `File` and every socket class. `class_id` is carried
/// explicitly because [`IoBackend::Uninit`] names no descriptor to derive it
/// from, and a blank `File` must still report `File`.
pub(crate) fn uninit_io(class_id: ClassId) -> RubyValue {
    let io = RIo::new(IoBackend::Uninit, None);
    io.class_override
        .store(class_id.0, std::sync::atomic::Ordering::Relaxed);
    RubyValue::Object(Arc::new(io))
}

pub(super) fn io_allocate() -> RubyValue {
    uninit_io(IO_CLASS)
}

/// The raw descriptor behind a socket-backed IO value (a `TCPSocket`/`Socket`/
/// ... created via [`socket_value`]), or `None` if the receiver is not an open
/// fd-backed IO. The fd stays owned by the value -- callers borrow it for
/// `libc` calls (`getsockname`, `setsockopt`, `send`, ...) and must not close
/// it. A closed socket answers `None`, so callers raise `IOError` on it.
pub(crate) fn socket_raw_fd(recv: &RubyValue) -> Option<std::os::fd::RawFd> {
    use std::os::fd::AsRawFd;
    let io = as_rio(recv)?;
    match &*io.backend.lock() {
        IoBackend::Pipe(Some(f)) | IoBackend::File(Some(f)) => Some(f.as_raw_fd()),
        _ => None,
    }
}

pub(super) fn std_io(stream: StdStream) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::Std(stream), None)))
}

/// Wrap an already-open file as a Ruby `File` value.
pub(crate) fn file_value(f: std::fs::File, path: Option<String>) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::File(Some(f)), path)))
}

/// [`file_value`] recording the MODE it was opened with, so a later
/// `#reopen(path)` with no mode of its own inherits it.
pub(crate) fn file_value_mode(
    f: std::fs::File,
    path: Option<String>,
    mode: Option<String>,
) -> RubyValue {
    let io = RIo::new(IoBackend::File(Some(f)), path);
    *io.open_mode.lock() = mode;
    RubyValue::Object(Arc::new(io))
}

/// A zeroed read buffer of `n` bytes, or `NoMemoryError`.
///
/// `f.read(10**18)` allocated eagerly with `vec![0u8; n]`, whose failure path
/// is `handle_alloc_error` -- an ABORT, not a rescuable exception. Ruby raises
/// `NoMemoryError: failed to allocate memory`, and so does this.
pub(super) fn read_buffer(n: usize) -> Result<Vec<u8>, Signal> {
    let mut buf: Vec<u8> = Vec::new();
    buf.try_reserve_exact(n).map_err(|_| {
        crate::dispatch::raise_error("NoMemoryError", "failed to allocate memory".to_string())
    })?;
    buf.resize(n, 0);
    Ok(buf)
}

/// How many bytes an `ioctl` request number says its argument buffer holds.
///
/// The request encodes the size in bits 16..29 on every platform zeo builds
/// for -- BSD's `IOCPARM_LEN` and Linux's `_IOC_SIZE` are the same field. A
/// request that encodes nothing answers 0, and the caller's own String length
/// stands.
pub(super) fn ioctl_param_len(request: libc::c_ulong) -> usize {
    ((request >> 16) & 0x1fff) as usize
}

/// Wrap one end of an `IO.pipe` (from an owned fd) as a Ruby `IO` value.
pub(crate) fn pipe_value(f: std::fs::File) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::Pipe(Some(f)), None)))
}

/// [`pipe_value`] with a `#path` -- a pty master, whose name is the slave's
/// device under CRuby's `masterpty:` prefix rather than a node of its own.
pub(crate) fn pipe_value_named(f: std::fs::File, path: String) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::Pipe(Some(f)), Some(path))))
}

pub(super) fn recv_io(recv: &RubyValue) -> Result<&RubyValue, Signal> {
    // The table only dispatches on IO_CLASS receivers, so `recv` is always
    // one of the singletons -- kept as a value so the write helpers stay
    // target-shaped.
    Ok(recv)
}

/// Whether this IO has been `#close`d -- the `IO::Buffer` entry points
/// raise IOError "closed stream" before touching the fd.
pub(crate) fn io_is_closed(recv: &RubyValue) -> bool {
    as_rio(recv).is_some_and(|io| {
        matches!(
            &*io.backend.lock(),
            IoBackend::File(None) | IoBackend::Pipe(None)
        )
    })
}

pub fn as_rio(recv: &RubyValue) -> Option<&RIo> {
    match recv {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RIo>(),
        _ => None,
    }
}

/// The `FMODE_*` word `rb_io_mode` answers a C extension, from what this
/// handle was opened with. The numbers are `ruby/io.h`'s own.
///
/// CRuby keeps this word in `fptr->mode` and builds it while parsing the mode
/// string; zeo keeps the mode STRING and rebuilds the word on demand, which is
/// why the parse lives here rather than at open time. The two agree on every
/// bit an extension can act on. The three CRuby sets that zeo cannot: `DUPLEX`
/// (zeo has no tied write half -- see `rb_io_set_write_io`), `TEXTMODE`
/// (nothing on a Unix stream distinguishes it from binary), and
/// `SETENC_BY_BOM` (a transient of the open, not kept).
pub fn fmode_bits(recv: &RubyValue) -> i32 {
    const READABLE: i32 = 0x0000_0001;
    const WRITABLE: i32 = 0x0000_0002;
    const BINMODE: i32 = 0x0000_0004;
    const SYNC: i32 = 0x0000_0008;
    const TTY: i32 = 0x0000_0010;
    const APPEND: i32 = 0x0000_0040;
    const CREATE: i32 = 0x0000_0080;
    const EXCL: i32 = 0x0000_0400;
    const TRUNC: i32 = 0x0000_0800;

    let Some(io) = as_rio(recv) else {
        return 0;
    };
    let recorded = io.open_mode.lock().clone();
    let mut bits = match recorded.as_deref() {
        // Everything after `:` names an encoding, not a mode.
        Some(mode) => {
            let letters = mode.split(':').next().unwrap_or("");
            let mut bits = match letters.as_bytes().first() {
                Some(b'r') => READABLE,
                Some(b'w') => WRITABLE | CREATE | TRUNC,
                Some(b'a') => WRITABLE | APPEND | CREATE,
                _ => READABLE,
            };
            for c in letters.bytes() {
                match c {
                    b'+' => bits |= READABLE | WRITABLE,
                    b'b' => bits |= BINMODE,
                    b'x' => bits |= EXCL,
                    _ => {}
                }
            }
            bits
        }
        // No mode was named: a std stream, or an IO over a descriptor someone
        // else opened. Its direction is the only thing that can be known.
        None => match stream_of(recv) {
            Some(StdStream::Stdin) => READABLE,
            Some(StdStream::Stdout | StdStream::Stderr) => WRITABLE,
            None => READABLE | WRITABLE,
        },
    };
    if io.binmode.load(std::sync::atomic::Ordering::Relaxed) {
        bits |= BINMODE;
    }
    if io.sync.load(std::sync::atomic::Ordering::Relaxed) {
        bits |= SYNC;
    }
    // Asked of the descriptor rather than of `#tty?`, which answers false for
    // anything that is not a std stream -- a pty a gem opened is a tty, and
    // this word is what a gem checks before turning on line buffering.
    if let Ok(RubyValue::Int(fd)) = fileno_value(recv)
        && let Ok(fd) = i32::try_from(fd)
        // SAFETY: a plain query on a descriptor this handle owns.
        && unsafe { libc::isatty(fd) } == 1
    {
        bits |= TTY;
    }
    bits
}

/// `#fileno`'s value -- also what `raw_fd` reads before narrowing it to a
/// `libc::c_int` for the ioctl/poll/termios calls.
pub(super) fn fileno_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    if let Some(io) = as_rio(recv) {
        // A pipe end and a socket carry a real descriptor too, not just a file.
        match &*io.backend.lock() {
            IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => {
                return Ok(RubyValue::Int(f.as_raw_fd() as i64));
            }
            IoBackend::Uninit => return Err(io_error!("uninitialized stream")),
            // `rb_io_descriptor` runs `GetOpenFile` first, so every caller
            // below -- the ioctls, the termios calls -- refuses a closed
            // stream rather than reaching whatever now owns descriptor 0.
            IoBackend::File(None) | IoBackend::Pipe(None) => {
                return Err(io_error!("closed stream"));
            }
            _ => {}
        }
    }
    Ok(RubyValue::Int(match stream_of(recv) {
        Some(StdStream::Stdin) => 0,
        Some(StdStream::Stdout) => 1,
        Some(StdStream::Stderr) => 2,
        None => 0,
    }))
}

/// The receiver's fd -- `#fileno` without the `RubyValue` round trip, for the
/// libc calls (`ioctl`, `poll`, `termios`) that need a raw descriptor.
pub(crate) fn raw_fd(recv: &RubyValue) -> Result<libc::c_int, Signal> {
    let RubyValue::Int(fd) = fileno_value(recv)? else {
        unreachable!("fileno_value answers an Int");
    };
    Ok(fd as libc::c_int)
}

/// How CRuby names a stream in an `Errno` message: `<STDIN>` and friends for a
/// std stream, the path for a file, empty for anything else.
pub(crate) fn stream_label(recv: &RubyValue) -> String {
    match stream_of(recv) {
        Some(StdStream::Stdin) => "<STDIN>".to_string(),
        Some(StdStream::Stdout) => "<STDOUT>".to_string(),
        Some(StdStream::Stderr) => "<STDERR>".to_string(),
        None => as_rio(recv)
            .and_then(|io| io.path.lock().clone())
            .unwrap_or_default(),
    }
}

pub(crate) fn stream_of(recv: &RubyValue) -> Option<StdStream> {
    match &*as_rio(recv)?.backend.lock() {
        IoBackend::Std(s) => Some(*s),
        IoBackend::File(_) | IoBackend::Pipe(_) | IoBackend::Uninit => None,
    }
}

/// Run `f` against the receiver's open file, or raise the IOError a closed
/// or non-file receiver deserves -- the shared preamble of every read/seek
/// row below. Runs Gvl-released: a pipe or socket read here can block
/// indefinitely, and an armed (`ZEO_GVL=1`) holder must not stall its
/// siblings behind it. The whole lock-op-unlock section releases as one
/// unit; contention on the SAME IO still serializes on its backend lock
/// (CRuby serializes per-fd operations too).
/// Which directions this descriptor is open for, read off `fcntl(F_GETFL)`.
///
/// The DESCRIPTOR, not a remembered mode string: only a mode string that was
/// NAMED was ever recorded, so `File.open(path)`, integer flags, `IO.new(fd)`,
/// a pipe end and a socket all had nothing to check against. `F_GETFL`
/// answers for all of them, including after a `reopen`'s dup2.
pub(super) fn access_mode(io: &RIo) -> Option<(bool, bool)> {
    use std::os::fd::AsRawFd;
    let fd = match &*io.backend.lock() {
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => f.as_raw_fd(),
        _ => return None,
    };
    // SAFETY: a plain query on a descriptor this handle owns.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return None;
    }
    match flags & libc::O_ACCMODE {
        libc::O_RDONLY => Some((true, false)),
        libc::O_WRONLY => Some((false, true)),
        _ => Some((true, true)),
    }
}

/// CRuby checks the mode BEFORE touching the descriptor, so a write-only
/// handle read from is an `IOError`, not the kernel's `EBADF`.
pub(crate) fn check_readable(recv: &RubyValue) -> Result<(), Signal> {
    match as_rio(recv).and_then(access_mode) {
        Some((false, _)) => Err(io_error!("not opened for reading")),
        _ => Ok(()),
    }
}

/// [`check_readable`]'s twin.
pub(crate) fn check_writable(recv: &RubyValue) -> Result<(), Signal> {
    match as_rio(recv).and_then(access_mode) {
        Some((_, false)) => Err(io_error!("not opened for writing")),
        _ => Ok(()),
    }
}

/// This IO's descriptor, or `None` for a std stream or a closed handle --
/// `fcntl` needs the real number.
pub(super) fn io_raw_fd(recv: &RubyValue) -> Option<std::os::fd::RawFd> {
    use std::os::fd::AsRawFd;
    let io = as_rio(recv)?;
    let backend = io.backend.lock();
    match &*backend {
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => Some(f.as_raw_fd()),
        _ => None,
    }
}
