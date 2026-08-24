//! `IO` (G0, minimal): the `STDOUT`/`STDERR` singletons and the
//! `$stdout`/`$stderr` globals the Kernel print family routes through.
//! File-backed IO, `STDIN`/`gets`, buffering modes, and encodings are a
//! later phase (plan P-B) -- this slice exists so `STDOUT.puts`,
//! `$stderr.print`, and `$stdout = <duck>` redirection behave.

use std::io::Write;
use std::sync::{Arc, LazyLock};

use crate::dispatch::{RObj, RubyObject};
use crate::signal::Signal;
use crate::value::RubyValue;
use zeo_abi::{ClassId, IO_CLASS};
use zeo_macros::ruby_class;

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
    fn close_file(&mut self) -> std::io::Result<()> {
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
            IoBackend::Std(_) => Ok(()),
        }
    }

    /// Give the descriptor up WITHOUT closing it -- what `autoclose = false`
    /// promises, since the number belongs to whoever handed it over.
    fn release_file(&mut self) {
        use std::os::fd::IntoRawFd;
        match self {
            IoBackend::File(slot) | IoBackend::Pipe(slot) => {
                if let Some(f) = slot.take() {
                    // Deliberately unclosed: the descriptor outlives us.
                    let _ = f.into_raw_fd();
                }
            }
            IoBackend::Std(_) => {}
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
    backend: parking_lot::Mutex<IoBackend>,
    /// The path this was opened from, for error messages and `#path`. `None`
    /// when there is no associated file (a pipe, a std stream, or a
    /// `File.new(fd)` given no `path:`) -- distinct from `Some("")`, which
    /// `File.new(fd, path: "")` produces and `#path` must answer as `""`.
    /// Behind a `Mutex` because `#reopen` REPLACES it: the receiver keeps its
    /// descriptor number and takes on the target file.
    path: parking_lot::Mutex<Option<String>>,
    /// The MODE string this handle was opened with, when a caller named one.
    /// `#reopen(path)` with no mode of its own inherits it, which is CRuby's
    /// rule (`rb_io_reopen` reuses `fptr->mode`) and the difference between
    /// `reopen`-ing a write handle and opening the target read-only.
    open_mode: parking_lot::Mutex<Option<String>>,
    /// `#lineno` -- the count of lines read via `gets`/`readline`/`each_line`,
    /// which CRuby tracks per-IO and lets a program set with `lineno=`.
    lineno: std::sync::atomic::AtomicI64,
    /// `#binmode?` -- on Unix binary vs text mode has no behavioural effect
    /// (no CRLF translation), so this only records what `#binmode` was told,
    /// exactly what `#binmode?` reports.
    binmode: std::sync::atomic::AtomicBool,
    /// `#autoclose?` -- whether closing this IO closes its fd. Defaults to
    /// true; a program may clear it (`autoclose = false`) to keep the fd open.
    autoclose: std::sync::atomic::AtomicBool,
    /// `#sync` -- whether a write reaches the descriptor at once. Every write
    /// here already does, so this only RECORDS what CRuby would report: true
    /// for STDERR, a socket, a popen handle, and a pipe's write end; false for
    /// a file, STDOUT, STDIN, and a pipe's read end.
    sync: std::sync::atomic::AtomicBool,
    /// `Kernel#freeze`'s own flag. It gates nothing -- what a handle holds
    /// is a descriptor, not a Ruby-visible field -- but `frozen?` answers
    /// what was written, which a hardcoded `false` did not.
    frozen: std::sync::atomic::AtomicBool,
    /// Bytes pushed back by `#ungetbyte`/`#ungetc`, read out (LIFO) before the
    /// stream itself. The next byte read drains this first.
    unget: parking_lot::Mutex<Vec<u8>>,
    /// Bytes read AHEAD of what Ruby has consumed -- see [`ReadBuf`]. Only the
    /// line/char/byte readers fill it; `with_file` gives every other row a
    /// descriptor positioned exactly where Ruby thinks it is.
    rbuf: parking_lot::Mutex<ReadBuf>,
    /// A socket reports `TCPSocket`/`TCPServer` (not `IO`) for `#class`, while
    /// still using a `Pipe`-shaped fd for read/write. `None` for ordinary
    /// files, pipes, and std streams (their class comes from the backend).
    class_override: Option<ClassId>,
    /// The child this IO is connected to, for an `IO.popen` handle (0 = none).
    /// `#pid` answers it, and `#close` reaps the child and sets `$?`.
    child_pid: std::sync::atomic::AtomicI64,
    /// `#external_encoding`/`#internal_encoding` once `set_encoding` has been
    /// told. Unset, a READABLE stream reports `Encoding.default_external` and
    /// a write-only one reports nil -- CRuby's rule, and why this cannot just
    /// default to the process encoding.
    encodings: parking_lot::Mutex<(
        Option<crate::encoding::EncodingId>,
        Option<crate::encoding::EncodingId>,
    )>,
    /// `#timeout`/`#timeout=` -- recorded and read back, nil by default. zeo's
    /// reads block, so nothing enforces it; see COMPATIBILITY.md.
    timeout: parking_lot::Mutex<RubyValue>,
}

/// The byte-order mark a stream opens with, if any: the encoding it names and
/// how many bytes it occupies. UTF-32's marks are checked before UTF-16's,
/// since `FF FE 00 00` starts with UTF-16LE's own mark.
fn read_bom(recv: &RubyValue) -> Result<Option<(crate::encoding::EncodingId, usize)>, Signal> {
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
/// Reading a line one `read(2)` at a time costs a syscall PER BYTE:
/// `bm_io_wordcount` spent 86% of its wall clock inside `read`, against a
/// CRuby that refills a buffer in chunks. Buffering here is the same trade,
/// with one rule that keeps it invisible: the descriptor sits AHEAD of the
/// position Ruby believes in by exactly `data.len() - pos` bytes, and
/// [`with_file`] seeks that difference back before handing the descriptor to
/// anything else. So `#read`, `#seek`, `#pos`, `#eof?`, `#sysread` and every
/// other row see precisely the file they saw before this existed, and the
/// buffer can only ever be filled where it can also be given back.
#[derive(Default)]
struct ReadBuf {
    data: Vec<u8>,
    /// How much of `data` the caller has already consumed.
    pos: usize,
    /// Whether the descriptor can seek, so the unconsumed tail can be
    /// returned. `None` until probed once; a pipe, socket or std stream
    /// answers `false` and never buffers at all.
    seekable: Option<bool>,
}

impl ReadBuf {
    /// Bytes read but not yet handed out -- how far the descriptor sits ahead.
    fn pending(&self) -> usize {
        self.data.len() - self.pos
    }
}

impl RubyObject for RIo {
    // A File instance carries FILE_CLASS so its own MRO (`File < IO`) finds
    // File's rows before IO's; the std streams are plain IOs.
    fn class_id(&self) -> ClassId {
        if let Some(c) = self.class_override {
            return c;
        }
        match &*self.backend.lock() {
            IoBackend::File(_) => zeo_abi::FILE_CLASS,
            IoBackend::Std(_) | IoBackend::Pipe(_) => IO_CLASS,
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
            IoBackend::File(_) | IoBackend::Pipe(_) => StdStream::Stdout,
        };
        Arc::new(RIo::new(IoBackend::Std(stream), self.path.lock().clone()))
    }
}

impl RIo {
    /// This handle's live descriptor, or `None` once it is closed. A std
    /// stream's is its well-known number.
    fn raw_fd(&self) -> Option<libc::c_int> {
        use std::os::fd::AsRawFd;
        match &*self.backend.lock() {
            IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => Some(f.as_raw_fd()),
            IoBackend::File(None) | IoBackend::Pipe(None) => None,
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
    fn dup2_from(&self, src: libc::c_int, path: Option<String>) -> Result<(), Signal> {
        use std::os::fd::FromRawFd;
        let Some(dst) = self.raw_fd() else {
            return Err(crate::dispatch::raise_error(
                "IOError",
                "closed stream".to_string(),
            ));
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
    fn new(backend: IoBackend, path: Option<String>) -> RIo {
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
            class_override: None,
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

fn reset_handle_state(io: &RIo) {
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
    io.class_override = Some(class_id);
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

fn std_io(stream: StdStream) -> RubyValue {
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

/// Wrap one end of an `IO.pipe` (from an owned fd) as a Ruby `IO` value.
pub(crate) fn pipe_value(f: std::fs::File) -> RubyValue {
    RubyValue::Object(Arc::new(RIo::new(IoBackend::Pipe(Some(f)), None)))
}

/// Mark a freshly created raw descriptor close-on-exec, as CRuby marks every
/// fd it creates (and as Rust's own `File` opens already are). A child must
/// receive only the stdio deliberately handed to it -- a pipe write end
/// leaking into an unrelated child holds the reader's EOF open forever, the
/// classic popen-family deadlock. `dup2` at exec time clears the flag on the
/// child's own stdio copies, so redirect targets still arrive open.
pub(crate) fn set_fd_cloexec(fd: libc::c_int) {
    // SAFETY: the caller just created `fd` and owns it.
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
}

/// Wrap an `IO.popen` handle: a pipe-backed IO carrying the child's pid, so
/// `#pid` answers it and `#close` reaps the child into `$?`.
pub(crate) fn popen_value(f: std::fs::File, pid: i64) -> RubyValue {
    let io = RIo::new(IoBackend::Pipe(Some(f)), None);
    io.child_pid
        .store(pid, std::sync::atomic::Ordering::Relaxed);
    // A popen handle is unbuffered, as CRuby's is.
    io.sync.store(true, std::sync::atomic::Ordering::Relaxed);
    RubyValue::Object(Arc::new(io))
}

/// Duplicate the descriptor behind an fd-backed IO value into an owned `File`
/// -- what a spawn redirect target (`out: pipe_w`, Open3's shape) hands the
/// child. The dup leaves the Ruby IO open and independent; `None` means the
/// value is not an open fd-backed IO (a filename target, or a StringIO).
pub(crate) fn dup_fd_file(v: &RubyValue) -> Option<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let io = as_rio(v)?;
    let fd = match &*io.backend.lock() {
        IoBackend::Pipe(Some(f)) | IoBackend::File(Some(f)) => f.as_raw_fd(),
        IoBackend::Std(StdStream::Stdin) => 0,
        IoBackend::Std(StdStream::Stdout) => 1,
        IoBackend::Std(StdStream::Stderr) => 2,
        IoBackend::Pipe(None) | IoBackend::File(None) => return None,
    };
    // SAFETY: `fd` is open (borrowed from the live backend above); `dup(2)`
    // hands back a fresh descriptor this File then owns.
    let dup = unsafe { libc::dup(fd) };
    if dup < 0 {
        return None;
    }
    // The child's stdio must be BLOCKING -- `IO.pipe` ends carry `O_NONBLOCK`,
    // and a child (`cat`, ...) fails on the resulting `EAGAIN`. CRuby's exec
    // machinery clears the flag on the child's fds 0..2 the same way (the
    // shared file description means the parent's end goes blocking too, there
    // as here; the parent's read rows handle both).
    set_fd_cloexec(dup);
    // SAFETY: `dup` is the fresh, owned descriptor just created.
    unsafe {
        let flags = libc::fcntl(dup, libc::F_GETFL);
        if flags >= 0 && flags & libc::O_NONBLOCK != 0 {
            libc::fcntl(dup, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
        Some(std::fs::File::from_raw_fd(dup))
    }
}

pub fn stdout_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stdout));
    V.clone()
}

pub fn stderr_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stderr));
    V.clone()
}

pub fn stdin_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stdin));
    V.clone()
}

/// Installs the `STDIN`/`STDOUT`/`STDERR` constants and the matching
/// `$stdin`/`$stdout`/`$stderr` globals -- called once from generated
/// `main()` (CRuby startup parity).
pub fn seed_stdio() {
    crate::constants::const_set(0, "STDOUT", stdout_value());
    crate::constants::const_set(0, "STDERR", stderr_value());
    crate::constants::const_set(0, "STDIN", stdin_value());
    crate::globals::seed_global(0, "$stdout", stdout_value());
    crate::globals::seed_global(0, "$stderr", stderr_value());
    crate::globals::seed_global(0, "$stdin", stdin_value());
    // `$>` is not a copy of `$stdout` but the SAME slot: assigning either
    // redirects both (oracle-verified in both directions). `PP.pp` defaults its
    // output to it, which is how a nil `$>` reached prettyprint as a receiver.
    crate::globals::global_alias(0, "$>", "$stdout");
}

/// The value `$stdout` currently holds in box 0 (nil -- never assigned --
/// means the default singleton). The print family targets this, so
/// `$stdout = STDERR` (or any duck-typed writer) redirects `puts`/`p`/....
/// Until some assignment has actually touched a stdio global
/// (`stdio_redirected`), the answer IS the seeded singleton -- returned
/// directly, skipping the alias-resolve and table locks per write call.
pub fn current_stdout() -> RubyValue {
    if !crate::globals::stdio_redirected() {
        return stdout_value();
    }
    match crate::globals::global_get(0, "$stdout") {
        RubyValue::Nil => stdout_value(),
        v => v,
    }
}

pub fn current_stderr() -> RubyValue {
    if !crate::globals::stdio_redirected() {
        return stderr_value();
    }
    match crate::globals::global_get(0, "$stderr") {
        RubyValue::Nil => stderr_value(),
        v => v,
    }
}

/// The `RIo` backend arms every writer shares -- BYTES in, so a BINARY
/// string's raw bytes reach the fd untouched (the display pipeline's
/// `to_utf8_lossy` promotes `0xB4` to `0xC2 0xB4`, which corrupted every
/// binary-image benchmark's output; see `write_value`).
fn write_rio(io: &RIo, bytes: &[u8]) -> Result<(), Signal> {
    // Gvl-released like `with_file`: a write to a full pipe blocks until
    // the reader drains it, and an armed holder must not stall siblings
    // behind that.
    crate::gvl::without_gvl(|| match &mut *io.backend.lock() {
        IoBackend::Std(StdStream::Stdout) => {
            let mut out = std::io::stdout();
            if out.write_all(bytes).is_err() {
                // A closed pipe downstream (`head`, etc.) -- CRuby
                // dies with EPIPE; a quiet exit is the pragmatic
                // equivalent here.
                std::process::exit(0);
            }
            Ok(())
        }
        IoBackend::Std(StdStream::Stderr) => {
            let _ = std::io::stderr().write_all(bytes);
            Ok(())
        }
        IoBackend::Std(StdStream::Stdin) => Err(io_error!("not opened for writing")),
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => blocking_write_all(f, bytes)
            .map_err(|e| {
                crate::builtins::file::raise_errno(
                    &e,
                    "write",
                    io.path.lock().as_deref().unwrap_or_default(),
                )
            }),
    })
}

/// Writes `s` to `target`: directly for one of our `RIo`s, via a dynamic
/// `write` send for anything else (`$stdout = <duck>` redirection --
/// CRuby's own contract is "any object responding to `write`").
pub fn write_str(target: &RubyValue, s: &str) -> Result<(), Signal> {
    if let RubyValue::Object(o) = target
        && let Some(io) = o.as_any().downcast_ref::<RIo>()
    {
        return write_rio(io, s.as_bytes());
    }
    crate::dispatch::send_value(
        target,
        crate::symbol::wk::write(),
        &[RubyValue::Str(crate::collections::string_new(
            s.to_string(),
        ))],
        None,
    )
    .map(|_| ())
}

/// `write_str` for an already-assembled BYTE buffer (the `print`/`puts`
/// accumulators, `putc`'s single byte). A duck target receives it as a
/// BINARY string -- the honest tag for bytes with no other provenance.
pub fn write_bytes(target: &RubyValue, bytes: &[u8]) -> Result<(), Signal> {
    if let RubyValue::Object(o) = target
        && let Some(io) = o.as_any().downcast_ref::<RIo>()
    {
        return write_rio(io, bytes);
    }
    crate::dispatch::send_value(
        target,
        crate::symbol::wk::write(),
        &[RubyValue::Str(crate::collections::string_from_bytes(
            bytes.to_vec(),
            crate::encoding::ASCII_8BIT,
        ))],
        None,
    )
    .map(|_| ())
}

/// Writes one Ruby VALUE the way `IO#write`/`#<<` must: a String
/// contributes its RAW bytes in its own encoding (a duck target gets the
/// very same String object, exactly CRuby); anything else goes through the
/// display rendering. Answers the BYTE count written (`IO#write`'s return
/// contract). This is the seam that keeps `0xB4` one byte instead of the
/// display pipeline's Latin-1 -> UTF-8 promotion.
pub fn write_value(target: &RubyValue, v: &RubyValue) -> Result<i64, Signal> {
    if let RubyValue::Str(s) = v {
        let bytes = {
            let b = s.lock();
            b.bytes().to_vec()
        };
        if let RubyValue::Object(o) = target
            && let Some(io) = o.as_any().downcast_ref::<RIo>()
        {
            write_rio(io, &bytes)?;
            return Ok(bytes.len() as i64);
        }
        crate::dispatch::send_value(
            target,
            crate::symbol::wk::write(),
            std::slice::from_ref(v),
            None,
        )?;
        return Ok(bytes.len() as i64);
    }
    let s = v.try_display_string()?;
    write_str(target, &s)?;
    Ok(s.len() as i64)
}

/// Appends `v`'s printed form to a BYTE buffer: a String's raw bytes in
/// its own encoding, every other value's display rendering (UTF-8). The
/// `print`/`puts` family accumulates through this so binary strings
/// survive to the fd byte-for-byte.
pub fn display_bytes(v: &RubyValue, buf: &mut Vec<u8>) -> Result<(), Signal> {
    match v {
        RubyValue::Str(s) => buf.extend_from_slice(s.lock().bytes()),
        // Fallible: a user `to_s` that raises propagates out of the
        // `print`/`puts` family as a catchable exception (CRuby's rule).
        other => buf.extend_from_slice(other.try_display_string()?.as_bytes()),
    }
    Ok(())
}

/// `puts`'s rendering into a BYTE buffer (a String arg contributes its raw
/// bytes -- see `display_bytes`): every arg on its own line, arrays
/// flattened recursively, `[...]` for a self-referential array, a bare
/// newline for no args / an empty array -- CRuby's exact shapes.
pub fn render_puts(args: &[RubyValue], buf: &mut Vec<u8>) -> Result<(), Signal> {
    fn put_one(v: &RubyValue, seen: &mut Vec<usize>, buf: &mut Vec<u8>) -> Result<(), Signal> {
        match v {
            RubyValue::Array(a) => {
                let id = Arc::as_ptr(a) as usize;
                if seen.contains(&id) {
                    buf.extend_from_slice(b"[...]\n");
                    return Ok(());
                }
                seen.push(id);
                let items = a.lock().clone();
                // An empty array contributes nothing (CRuby's `io_puts_ary`
                // loops zero times); only zero-arg `puts` writes a bare
                // newline -- oracle-verified `puts []` prints nothing.
                for e in &items {
                    put_one(e, seen, buf)?;
                }
                seen.pop();
            }
            other => {
                let start = buf.len();
                display_bytes(other, buf)?;
                if buf.len() == start || buf.last() != Some(&b'\n') {
                    buf.push(b'\n');
                }
            }
        }
        Ok(())
    }
    if args.is_empty() {
        buf.push(b'\n');
    }
    // One reusable cycle-guard: it is empty between top-level args by
    // construction (push/pop pairs), so sharing it never links siblings.
    let mut seen = Vec::new();
    for a in args {
        put_one(a, &mut seen, buf)?;
    }
    Ok(())
}

fn recv_io(recv: &RubyValue) -> Result<&RubyValue, Signal> {
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

pub(crate) fn as_rio(recv: &RubyValue) -> Option<&RIo> {
    match recv {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RIo>(),
        _ => None,
    }
}

/// `#fileno`'s value -- also what `raw_fd` reads before narrowing it to a
/// `libc::c_int` for the ioctl/poll/termios calls.
fn fileno_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    if let Some(io) = as_rio(recv) {
        // A pipe end and a socket carry a real descriptor too, not just a file.
        if let IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) = &*io.backend.lock() {
            return Ok(RubyValue::Int(f.as_raw_fd() as i64));
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

fn stream_of(recv: &RubyValue) -> Option<StdStream> {
    match &*as_rio(recv)?.backend.lock() {
        IoBackend::Std(s) => Some(*s),
        IoBackend::File(_) | IoBackend::Pipe(_) => None,
    }
}

/// Shared body of `IO#wait_readable`/`#wait_writable` (from `require "io/wait"`)
/// -- block until the stream is ready for `events` (`POLLIN`/`POLLOUT`) or the
/// optional `timeout` (seconds; `nil`/absent = block indefinitely) elapses.
/// Answers `self` when ready, `nil` on timeout, via real `poll(2)` over the fd
/// -- the same libc-over-`io_fileno` shape as `io_winsize`.
fn io_wait_for(
    recv: &RubyValue,
    args: &[RubyValue],
    events: libc::c_short,
) -> Result<RubyValue, Signal> {
    let timeout_ms = wait_timeout_ms(args.first());
    let mut pfd = libc::pollfd {
        fd: raw_fd(recv)?,
        events,
        revents: 0,
    };
    unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    Ok(if poll_ready(pfd.revents, events) {
        recv.clone()
    } else {
        RubyValue::Nil
    })
}

/// Whether one `poll(2)` result counts as ready for `events`. A hangup or an
/// error makes a stream readable (at EOF) and writable (the write reports the
/// error), but never PRIORITY-readable: only out-of-band data is that.
/// A readiness timeout in seconds as milliseconds. Absent or nil blocks
/// forever (`-1`); anything at or below zero polls and returns at once.
fn wait_timeout_ms(timeout: Option<&RubyValue>) -> libc::c_int {
    match timeout {
        None | Some(RubyValue::Nil) => -1,
        Some(v) => {
            let secs = crate::builtins::numeric::num_to_f64_unchecked(v);
            if secs <= 0.0 {
                0
            } else {
                (secs * 1000.0) as libc::c_int
            }
        }
    }
}

fn poll_ready(revents: libc::c_short, events: libc::c_short) -> bool {
    if revents & events != 0 {
        return true;
    }
    events != libc::POLLPRI && revents & (libc::POLLHUP | libc::POLLERR) != 0
}

/// One `select(2)` answer: a ready flag per descriptor, per set, in the order
/// the sets were given.
type Readiness = (Vec<bool>, Vec<bool>, Vec<bool>);

/// Which of `fds` are ready, one `select(2)` per set. `poll(2)` cannot answer
/// this: on macOS it reports `POLLPRI` for any readable pipe, so an
/// exception-set query there would claim ordinary bytes are out-of-band data.
/// `select` is also what CRuby's own `IO.select` calls.
///
/// A descriptor at or past `FD_SETSIZE` cannot be named in an `fd_set` and is
/// reported not-ready rather than corrupting the set.
fn select_ready(
    read: &[libc::c_int],
    write: &[libc::c_int],
    except: &[libc::c_int],
    timeout_ms: libc::c_int,
) -> Result<Readiness, Signal> {
    const LIMIT: libc::c_int = libc::FD_SETSIZE as libc::c_int;
    // SAFETY: `fd_set` is a plain bitmap; zeroed is the empty set, which is
    // exactly what `FD_ZERO` writes.
    let mut sets: [libc::fd_set; 3] = unsafe { std::mem::zeroed() };
    let mut nfds = 0;
    for (set, fds) in sets.iter_mut().zip([read, write, except]) {
        for &fd in fds {
            if (0..LIMIT).contains(&fd) {
                unsafe { libc::FD_SET(fd, set) };
                nfds = nfds.max(fd + 1);
            }
        }
    }
    let mut tv = libc::timeval {
        tv_sec: (timeout_ms / 1000) as libc::time_t,
        tv_usec: (timeout_ms % 1000 * 1000) as libc::suseconds_t,
    };
    let deadline = if timeout_ms < 0 {
        std::ptr::null_mut()
    } else {
        &mut tv
    };
    let n = unsafe { libc::select(nfds, &mut sets[0], &mut sets[1], &mut sets[2], deadline) };
    if n < 0 {
        return Err(crate::builtins::file::raise_errno(
            &std::io::Error::last_os_error(),
            "select",
            "",
        ));
    }
    let mut out = Vec::new();
    for (set, fds) in sets.iter().zip([read, write, except]) {
        out.push(
            fds.iter()
                .map(|&fd| (0..LIMIT).contains(&fd) && unsafe { libc::FD_ISSET(fd, set) })
                .collect::<Vec<bool>>(),
        );
    }
    let mut it = out.into_iter();
    Ok((
        it.next().unwrap_or_default(),
        it.next().unwrap_or_default(),
        it.next().unwrap_or_default(),
    ))
}

/// Run `f` against the receiver's open file, or raise the IOError a closed
/// or non-file receiver deserves -- the shared preamble of every read/seek
/// row below. Runs Gvl-released: a pipe or socket read here can block
/// indefinitely, and an armed (`ZEO_GVL=1`) holder must not stall its
/// siblings behind it. The whole lock-op-unlock section releases as one
/// unit; contention on the SAME IO still serializes on its backend lock
/// (CRuby serializes per-fd operations too).
pub(crate) fn with_file<T>(
    recv: &RubyValue,
    f: impl FnOnce(&mut std::fs::File, &str) -> Result<T, Signal>,
) -> Result<T, Signal> {
    let Some(io) = as_rio(recv) else {
        return Err(io_error!("not a file"));
    };
    let path = io.path.lock().clone().unwrap_or_default();
    crate::gvl::without_gvl(|| match &mut *io.backend.lock() {
        IoBackend::File(Some(file)) | IoBackend::Pipe(Some(file)) => {
            // Give back whatever the line readers read ahead, so this closure
            // sees the descriptor at the position Ruby believes in. A no-op --
            // and syscall-free -- for any program that never buffered.
            unread(io, file);
            f(file, &path)
        }
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::Std(_) => Err(io_error!("not a file")),
    })
}

/// [`with_file`] for the rows that READ THROUGH the buffer rather than around
/// it: same locking and Gvl release, but no `unread` on the way in, and the
/// `RIo` is passed along so the closure can reach the buffer.
fn with_buffered_file<T>(
    recv: &RubyValue,
    f: impl FnOnce(&RIo, &mut std::fs::File, &str) -> Result<T, Signal>,
) -> Result<T, Signal> {
    let Some(io) = as_rio(recv) else {
        return Err(io_error!("not a file"));
    };
    let path = io.path.lock().clone().unwrap_or_default();
    crate::gvl::without_gvl(|| match &mut *io.backend.lock() {
        IoBackend::File(Some(file)) | IoBackend::Pipe(Some(file)) => f(io, file, &path),
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::Std(_) => Err(io_error!("not a file")),
    })
}

/// Rewind the descriptor over bytes [`ReadBuf`] read ahead, and drop them.
///
/// Infallible by construction: the buffer is only ever filled after `fill`
/// proved the descriptor seeks, so the seek back cannot be the first one to
/// fail. Should it fail anyway the buffer is still cleared, which loses the
/// read-ahead rather than serving it at a position it no longer matches.
fn unread(io: &RIo, f: &mut std::fs::File) {
    let mut buf = io.rbuf.lock();
    let pending = buf.pending();
    if pending > 0 {
        use std::io::Seek;
        let _ = f.seek(std::io::SeekFrom::Current(-(pending as i64)));
    }
    buf.data.clear();
    buf.pos = 0;
}

/// How much the buffer reads ahead. One page-ish chunk, matching CRuby's own
/// `IO` buffer size.
const READ_BUF: usize = 8192;

/// The next byte Ruby should see, drawing from [`ReadBuf`] and refilling it in
/// `READ_BUF` chunks. `None` at end of file.
///
/// The only place the buffer is filled, and it refuses to fill a descriptor
/// that cannot seek -- a pipe, socket or std stream keeps the byte-at-a-time
/// reads, where read-ahead could not be given back and `#readpartial`'s
/// arrival-shaped semantics would change.
fn buffered_byte(io: &RIo, f: &mut std::fs::File) -> std::io::Result<Option<u8>> {
    let mut buf = io.rbuf.lock();
    if buf.pos == buf.data.len() {
        let seekable = match buf.seekable {
            Some(s) => s,
            None => {
                use std::io::Seek;
                let s = f.stream_position().is_ok();
                buf.seekable = Some(s);
                s
            }
        };
        if !seekable {
            let mut one = [0u8; 1];
            return Ok((blocking_read(f, &mut one)? == 1).then_some(one[0]));
        }
        buf.data.resize(READ_BUF, 0);
        let got = blocking_read(f, &mut buf.data)?;
        buf.data.truncate(got);
        buf.pos = 0;
        if got == 0 {
            return Ok(None);
        }
    }
    let b = buf.data[buf.pos];
    buf.pos += 1;
    Ok(Some(b))
}

/// Park until `fd` is ready for `events`, the way CRuby's `rb_io_wait_readable`
/// / `rb_io_wait_writable` do. No timeout -- the caller asked for a BLOCKING
/// operation, and readiness is the only thing it is waiting on.
fn wait_ready(f: &std::fs::File, events: libc::c_short) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut pfd = libc::pollfd {
        fd: f.as_raw_fd(),
        events,
        revents: 0,
    };
    loop {
        // SAFETY: one initialized `pollfd` describing a descriptor this `File`
        // owns and keeps alive across the call.
        if unsafe { libc::poll(&mut pfd, 1, -1) } >= 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// One `read(2)` with BLOCKING semantics over a descriptor that may carry
/// `O_NONBLOCK` -- which both ends of an `IO.pipe` do, exactly as CRuby marks
/// its own. `EAGAIN` on such a descriptor means "nothing yet", not an error,
/// so this parks in `poll(2)` and retries rather than surfacing it; `EINTR` is
/// the ordinary retry. Every blocking read row goes through here, because a
/// user's own `io.nonblock = true` must not change what `#read` means either.
fn blocking_read(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::io::ErrorKind::{Interrupted, WouldBlock};
    loop {
        match std::io::Read::read(f, buf) {
            Err(e) if e.kind() == Interrupted => continue,
            Err(e) if e.kind() == WouldBlock => wait_ready(f, libc::POLLIN)?,
            other => return other,
        }
    }
}

/// [`blocking_read`]'s write twin: `write_all` over a descriptor that may be
/// non-blocking, waiting for room in a full pipe instead of failing.
fn blocking_write_all(f: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::ErrorKind::{Interrupted, WouldBlock, WriteZero};
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::io::Write::write(f, rest) {
            Ok(0) => return Err(WriteZero.into()),
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == Interrupted => continue,
            Err(e) if e.kind() == WouldBlock => wait_ready(f, libc::POLLOUT)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn io_read_val(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // A read from STDIN reads the real one; anything else needs a file.
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut buf = String::new();
        crate::gvl::without_gvl(|| std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "read", "<STDIN>"))?;
        return Ok(RubyValue::Str(crate::collections::string_new(buf)));
    }
    let n = match args.first() {
        None | Some(RubyValue::Nil) => None,
        Some(v) => match convert::to_index(v)? {
            i if i >= 0 => Some(i as usize),
            i => return Err(arg_error!("negative length {i} given")),
        },
    };
    // The encoding a WHOLE read tags its bytes with: the handle's external one
    // when it has been set (`File.open(path, "rb")`, an `encoding:` option, a
    // BOM), else UTF-8. It used to be UTF-8 unconditionally, so a binary
    // handle came back mis-tagged.
    let read_enc = as_rio(recv)
        .and_then(|io| io.encodings.lock().0)
        .unwrap_or(crate::encoding::UTF_8);
    with_file(recv, |f, path| {
        // A socket peer that closes with unread data sends RST, so a read can
        // return ECONNRESET AFTER delivering the bytes already buffered; CRuby
        // keeps that data and treats the reset as EOF. It never arises for a
        // regular file, so treating it as end here is harmless off a socket.
        // (EINTR/EAGAIN retries live in `blocking_read`.)
        use std::io::ErrorKind::ConnectionReset;
        match n {
            None => {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    match blocking_read(f, &mut chunk) {
                        Ok(0) => break,
                        Ok(k) => buf.extend_from_slice(&chunk[..k]),
                        Err(e) if e.kind() == ConnectionReset => break,
                        Err(e) => return Err(crate::builtins::file::raise_errno(&e, "read", path)),
                    }
                }
                // TAG the bytes, never re-encode them. Decoding a read as
                // UTF-8 replaced every non-UTF-8 byte with U+FFFD, so a file
                // read through `IO#read` (as opposed to `File.binread`, which
                // was already byte-faithful) came back corrupted -- see the
                // same rule on `write_rio` above.
                Ok(RubyValue::Str(crate::string_from_bytes(buf, read_enc)))
            }
            Some(n) => {
                let mut buf = vec![0u8; n];
                let mut got = 0;
                // `read` can answer short without being at EOF; loop until
                // the request is filled or the file genuinely ends.
                while got < n {
                    match blocking_read(f, &mut buf[got..]) {
                        Ok(0) => break,
                        Ok(k) => got += k,
                        Err(e) if e.kind() == ConnectionReset => break,
                        Err(e) => return Err(crate::builtins::file::raise_errno(&e, "read", path)),
                    }
                }
                buf.truncate(got);
                // EOF + a lengthed read is nil, NOT "" -- the asymmetry a
                // `while chunk = f.read(n)` loop relies on to terminate.
                if got == 0 && n > 0 {
                    return Ok(RubyValue::Nil);
                }
                // A LENGTHED read is binary in CRuby -- a byte count can land
                // mid-character, so there is nothing else it could honestly be.
                Ok(RubyValue::Str(crate::string_from_bytes(
                    buf,
                    crate::encoding::ASCII_8BIT,
                )))
            }
        }
    })
}

/// How `gets`/`readline`/`each_line`/`readlines` split their input: the line
/// separator (`None` = slurp the whole rest, i.e. `gets(nil)`), an optional
/// byte limit, and whether to strip the terminator (`chomp:`).
pub(crate) struct LineOpts {
    pub sep: Option<Vec<u8>>,
    pub limit: Option<usize>,
    pub chomp: bool,
}

/// Parse the shared `(sep = $/, limit = nil, chomp: false)` argument shape.
/// A leading Integer is the limit (separator stays `"\n"`); a leading String
/// is the separator, with an Integer that follows as the limit; a leading nil
/// slurps. The trailing keyword Hash carries `chomp:`.
pub(crate) fn line_opts(args: &[RubyValue]) -> LineOpts {
    let mut sep: Option<Vec<u8>> = Some(b"\n".to_vec());
    let mut limit = None;
    let mut chomp = false;
    let mut positional = args;
    if let Some(RubyValue::Hash(h)) = args.last() {
        chomp = crate::collections::hash_get(h, &RubyValue::Symbol(crate::symbol::wk::chomp()))
            .truthy();
        positional = &args[..args.len() - 1];
    }
    match positional.first() {
        Some(RubyValue::Int(n)) => limit = Some((*n).max(0) as usize),
        Some(RubyValue::Nil) => sep = None,
        Some(RubyValue::Str(s)) => sep = Some(s.lock().bytes().to_vec()),
        _ => {}
    }
    if let Some(RubyValue::Int(n)) = positional.get(1) {
        limit = Some((*n).max(0) as usize);
    }
    LineOpts { sep, limit, chomp }
}

/// Read the next line's bytes: up to and including the separator, or `limit`
/// bytes, or EOF. An empty result means EOF. Byte-at-a-time so the position
/// lands exactly after the line (a buffered read would desync `tell`).
fn read_line_bytes(io: &RIo, f: &mut std::fs::File, opts: &LineOpts) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        if let Some(lim) = opts.limit
            && out.len() >= lim
        {
            break;
        }
        match buffered_byte(io, f)? {
            None => break,
            Some(b) => {
                out.push(b);
                if let Some(s) = &opts.sep
                    && !s.is_empty()
                    && out.ends_with(s)
                {
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// Turn a line's bytes into the String `gets` answers, honoring `chomp:`.
fn line_string(mut bytes: Vec<u8>, opts: &LineOpts) -> RubyValue {
    chomp_line(&mut bytes, opts);
    RubyValue::Str(crate::collections::string_new(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

/// `chomp:`'s rule, on the raw bytes so an encoding-carrying caller
/// (`StringIO`) can apply it without going through UTF-8: strip one trailing
/// custom separator, then any trailing `"\r\n"`/`"\n"`/`"\r"`.
pub(crate) fn chomp_line(bytes: &mut Vec<u8>, opts: &LineOpts) {
    if !opts.chomp {
        return;
    }
    if let Some(sep) = &opts.sep
        && !sep.is_empty()
        && bytes.ends_with(sep)
    {
        bytes.truncate(bytes.len() - sep.len());
    }
    while bytes.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        bytes.pop();
    }
}

fn bump_lineno(recv: &RubyValue) {
    if let Some(io) = as_rio(recv) {
        io.lineno.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Read the next whole UTF-8 char from `f` (1-4 bytes by the lead byte), or
/// `None` at EOF.
fn read_one_char(io: &RIo, f: &mut std::fs::File) -> std::io::Result<Option<String>> {
    let Some(b0) = buffered_byte(io, f)? else {
        return Ok(None);
    };
    let n = if b0 < 0x80 {
        1
    } else if b0 >> 5 == 0b110 {
        2
    } else if b0 >> 4 == 0b1110 {
        3
    } else if b0 >> 3 == 0b11110 {
        4
    } else {
        1
    };
    let mut buf = vec![b0];
    for _ in 1..n {
        let Some(b) = buffered_byte(io, f)? else {
            break;
        };
        buf.push(b);
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

use crate::builtins::{arg_error, convert, eof_error, io_error, not_impl_error, type_error};
use std::sync::atomic::Ordering::Relaxed;

/// An integer argument (`pread`/`pwrite` counts, `fcntl`/`chmod` operands)
/// through the `to_int` protocol -- unlike the offset sites, a nil here is
/// the generic "of nil into Integer" (oracle-verified).
pub(crate) fn int_of(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Nil => Err(type_error!("no implicit conversion of nil into Integer")),
        v => convert::to_index(v),
    }
}

/// A byte-offset argument (`seek`/`sysseek`/`pos=`/`truncate`, `pread`'s
/// offset): CRuby's NUM2OFFT, whose nil TypeError is the bare
/// "no implicit conversion from nil" (no "to integer" -- oracle-verified).
pub(crate) fn offset_of(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Nil => Err(type_error!("no implicit conversion from nil")),
        v => convert::to_index(v),
    }
}

/// `seek`/`sysseek`'s whence: the SET/CUR/END symbols map to their
/// constants (CRuby's interpret_seek_whence); anything else -- unknown
/// symbols included -- goes through NUM2LONG (oracle: `seek(0, :BAD)` is
/// "no implicit conversion of Symbol into Integer").
fn whence_of(v: &RubyValue) -> Result<i64, Signal> {
    if let RubyValue::Symbol(s) = v {
        match s.name().as_str() {
            "SET" => return Ok(0),
            "CUR" => return Ok(1),
            "END" => return Ok(2),
            _ => {}
        }
    }
    convert::to_index(v)
}

/// This IO's descriptor, or `None` for a std stream or a closed handle --
/// `fcntl` needs the real number.
fn io_raw_fd(recv: &RubyValue) -> Option<std::os::fd::RawFd> {
    use std::os::fd::AsRawFd;
    let io = as_rio(recv)?;
    let backend = io.backend.lock();
    match &*backend {
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => Some(f.as_raw_fd()),
        _ => None,
    }
}

/// Turns `O_NONBLOCK` on or off for `fd`.
pub(crate) fn set_fd_nonblock(fd: std::os::fd::RawFd, on: bool) -> Result<(), Signal> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io_error!("closed stream"));
    }
    let next = if on {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };
    if unsafe { libc::fcntl(fd, libc::F_SETFL, next) } < 0 {
        return Err(crate::builtins::file::raise_errno(
            &std::io::Error::last_os_error(),
            "fcntl",
            "",
        ));
    }
    Ok(())
}

/// Whether this IO's fd was opened for writing (`close_write` needs it) or
/// reading (`close_read`). `fcntl(F_GETFL) & O_ACCMODE` is the fd's own truth,
/// so no per-IO mode field is needed.
fn fd_access_mode(recv: &RubyValue) -> Option<libc::c_int> {
    use std::os::fd::AsRawFd;
    let io = as_rio(recv)?;
    match &*io.backend.lock() {
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => {
            let flags = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) };
            (flags >= 0).then_some(flags & libc::O_ACCMODE)
        }
        _ => None,
    }
}

/// The bytes `putc` writes for its argument: a String's FIRST CHARACTER in
/// the string's own encoding (one raw byte for the byte encodings -- never
/// the display pipeline's Latin-1 -> UTF-8 promotion), an Integer's low
/// byte. NUM2CHR for everything else (`putc 2.5` truncates; a `to_str`
/// duck does NOT apply here -- oracle-verified).
pub(crate) fn putc_bytes(arg: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(match arg {
        RubyValue::Str(s) => {
            let b = s.lock();
            match b.encoding().kind() {
                crate::encoding::EncKind::Latin1
                | crate::encoding::EncKind::Binary
                | crate::encoding::EncKind::Registered
                | crate::encoding::EncKind::SingleByte => {
                    b.bytes().first().map(|&x| vec![x]).unwrap_or_default()
                }
                // First CHARACTER in the string's own encoding -- a
                // multibyte sequence stays its raw bytes.
                crate::encoding::EncKind::MultiByte(_)
                | crate::encoding::EncKind::Utf16 { .. }
                | crate::encoding::EncKind::Utf32 { .. } => {
                    b.char_at(0).map(|c| c.bytes().to_vec()).unwrap_or_default()
                }
                crate::encoding::EncKind::Utf8 | crate::encoding::EncKind::Ascii => b
                    .to_utf8_lossy()
                    .chars()
                    .next()
                    .map(|c| c.to_string().into_bytes())
                    .unwrap_or_default(),
            }
        }
        other => vec![(convert::to_index(other)? & 0xff) as u8],
    })
}

/// The `exception:` keyword of the non-blocking family. True (the default)
/// RAISES on a would-block or an EOF; false answers a `:wait_*` Symbol or nil
/// instead.
pub(crate) fn nonblock_raises(opts: Option<&RubyValue>) -> bool {
    let Some(RubyValue::Hash(h)) = opts else {
        return true;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern("exception"));
    !matches!(
        crate::collections::hash_get(h, &key),
        RubyValue::Bool(false)
    )
}

/// What a non-blocking operation answers when it would block: the matching
/// `IO::EAGAINWait*` exception, or the `:wait_readable`/`:wait_writable` Symbol
/// under `exception: false`. The exception INCLUDES `IO::WaitReadable`/
/// `IO::WaitWritable`, which is what a retry loop rescues.
pub(crate) fn would_block(write: bool, raises: bool, ctx: &str) -> Result<RubyValue, Signal> {
    let symbol = if write {
        "wait_writable"
    } else {
        "wait_readable"
    };
    if !raises {
        return Ok(RubyValue::Symbol(crate::Symbol::intern(symbol)));
    }
    let class = if write {
        "IO::EAGAINWaitWritable"
    } else {
        "IO::EAGAINWaitReadable"
    };
    Err(crate::dispatch::raise_error(
        class,
        format!("Resource temporarily unavailable - {ctx} would block"),
    ))
}

/// Whether `v` is an IO-like object (`IO`/`File`/`StringIO`) rather than a
/// filename. CRuby's `copy_stream` uses an IO argument at its CURRENT position;
/// only a String/`to_path` argument names a file to open.
fn is_io_object(v: &RubyValue) -> bool {
    matches!(
        v,
        RubyValue::Object(o)
            if matches!(
                o.class_id(),
                IO_CLASS | zeo_abi::FILE_CLASS | zeo_abi::STRINGIO_CLASS
            )
    )
}

/// `#gets`'s value, shared with the rows that drain through it
/// (`readline`, `readlines`, `each_line`).
fn gets_value(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let opts = line_opts(args);
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut line = String::new();
        let n = crate::gvl::without_gvl(|| {
            std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
        })
        .map_err(|e| crate::builtins::file::raise_errno(&e, "gets", "<STDIN>"))?;
        if n == 0 {
            return Ok(RubyValue::Nil);
        }
        bump_lineno(recv);
        return Ok(line_string(line.into_bytes(), &opts));
    }
    let line = with_buffered_file(recv, |io, f, path| {
        read_line_bytes(io, f, &opts)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "gets", path))
    })?;
    if line.is_empty() {
        return Ok(RubyValue::Nil);
    }
    bump_lineno(recv);
    Ok(line_string(line, &opts))
}

/// `#getc`'s value -- `#readchar` is this plus an EOF raise.
fn getc_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let ch = with_buffered_file(recv, |io, f, path| {
        read_one_char(io, f).map_err(|e| crate::builtins::file::raise_errno(&e, "getc", path))
    })?;
    Ok(match ch {
        Some(s) => RubyValue::Str(crate::collections::string_new(s)),
        None => RubyValue::Nil,
    })
}

/// `#getbyte`'s value -- `#readbyte` is this plus an EOF raise.
fn getbyte_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    // A byte pushed back with `#ungetbyte` is returned before the stream.
    if let Some(io) = as_rio(recv)
        && let Some(byte) = io.unget.lock().pop()
    {
        return Ok(RubyValue::Int(byte as i64));
    }
    let b = with_buffered_file(recv, |io, f, path| {
        buffered_byte(io, f).map_err(|e| crate::builtins::file::raise_errno(&e, "getbyte", path))
    })?;
    Ok(match b {
        Some(byte) => RubyValue::Int(byte as i64),
        None => RubyValue::Nil,
    })
}

/// An `fstat(2)` snapshot of the open descriptor, as a `File::Stat` --
/// what `#stat`, `#mtime` and `#size` all read.
pub(crate) fn stat_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    with_file(recv, |f, _path| {
        crate::builtins::stat::stat_from_fd(f.as_raw_fd())
    })
}

/// `#close`'s work, also run when `IO.pipe`/`IO.popen` close a handle they
/// own after a block.
fn close_io(recv: &RubyValue) -> Result<RubyValue, Signal> {
    if let Some(io) = as_rio(recv) {
        // `autoclose = false` keeps the descriptor open for its owner; the
        // handle still becomes closed either way.
        let mut backend = io.backend.lock();
        // `release_file` hands the descriptor back to its owner, so it must be
        // handed back at the position Ruby consumed to, not wherever the read
        // buffer left it.
        if let IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) = &mut *backend {
            unread(io, f);
        }
        // A close(2) that fails is ruby's to report: closing a descriptor a
        // second IO over the same fd already closed raises `Errno::EBADF,
        // "Bad file descriptor"` -- the bare strerror, with none of the
        // `@ syscall - path` suffix an open failure carries.
        let closed = if io.autoclose.load(std::sync::atomic::Ordering::Relaxed) {
            backend.close_file()
        } else {
            backend.release_file();
            Ok(())
        };
        drop(backend);
        if let Err(e) = closed {
            return Err(crate::builtins::file::raise_bare_errno(&e));
        }
        let pid = io.child_pid.swap(0, std::sync::atomic::Ordering::Relaxed);
        if pid != 0
            && let Ok(Some((reaped, raw))) =
                crate::gvl::without_gvl(|| crate::builtins::process::raw_waitpid(pid, 0))
        {
            crate::builtins::process::set_last_child_status(crate::builtins::process::new_status(
                reaped, raw,
            ));
        }
    }
    Ok(RubyValue::Nil)
}

/// `#autoclose=`'s store, also applied by `IO.new(fd, autoclose: false)`.
fn set_autoclose(recv: &RubyValue, on: &RubyValue) {
    if let Some(io) = as_rio(recv) {
        io.autoclose.store(on.truthy(), Relaxed);
    }
}

/// The whole-file class methods (`IO.read`, `IO.foreach`, ...) are identical to
/// `File`'s -- run File's own row rather than restating it.
fn file_class_row(
    name: &str,
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let row = crate::builtins::file::lookup_class(name)
        .unwrap_or_else(|| unreachable!("File.{name} is a row"));
    row(recv, args, block)
}

ruby_class! {
    IO = zeo_abi::IO_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    seed seed_io_constants;
    seed seed_stdio;
    // The open/lock flags, shared with `File` -- see `file.rs`. Listed after
    // Enumerable, so the resulting `IO.ancestors` is CRuby's
    // `[IO, File::Constants, Enumerable, ...]`.
    include zeo_abi::FILE_CONSTANTS_MODULE;

    def "puts" (recv, *args, &_blk) {
        let mut buf = Vec::new();
        // Flush-then-propagate on a raising `to_s` -- see `kernel_puts`.
        let rendered = render_puts(args, &mut buf);
        write_bytes(recv_io(recv)?, &buf)?;
        rendered?;
        Ok(RubyValue::Nil)
    }

    // The arguments are joined by `$,` and closed by `$\`, both nil (so
    // both empty) unless the program sets them -- `Kernel#print` is the
    // same body over `$stdout`, and neither reads them at stream creation.
    def "print" (recv, *args, &_blk) {
        let mut buf = Vec::new();
        let mut rendered = Ok(());
        let field_sep = crate::builtins::kernel::output_separator("$,");
        for (i, a) in args.iter().enumerate() {
            if i > 0 && let Some(s) = &field_sep {
                buf.extend_from_slice(s.bytes());
            }
            if let Err(sig) = display_bytes(a, &mut buf) {
                rendered = Err(sig);
                break;
            }
        }
        if rendered.is_ok()
            && let Some(s) = crate::builtins::kernel::output_separator("$\\")
        {
            buf.extend_from_slice(s.bytes());
        }
        // Flush-then-propagate on a raising `to_s` -- see `kernel_puts`.
        write_bytes(recv_io(recv)?, &buf)?;
        rendered?;
        Ok(RubyValue::Nil)
    }

    def "write" (recv, *args, &_blk) {
        let mut total = 0i64;
        for a in args {
            total += write_value(recv_io(recv)?, a)?;
        }
        Ok(RubyValue::Int(total))
    }

    def "<<" (recv, value, &_blk) {
        write_value(recv_io(recv)?, value)?;
        Ok(recv.clone())
    }

    // `#syswrite(str)` -- one unbuffered write, answering the byte count.
    // Every write here already reaches the descriptor at once, so this is
    // `#write` narrowed to a single argument, which is what CRuby's takes.
    def "syswrite" (recv, value, &_blk) {
        Ok(RubyValue::Int(write_value(recv_io(recv)?, value)?))
    }

    // `#ioctl(cmd, arg = 0)` -- the raw `ioctl(2)`. An Integer `arg` passes by
    // value; a String passes its buffer, which the call may WRITE THROUGH (the
    // whole point of the String form: the answer comes back in the buffer).
    def "ioctl" cfunc (recv, cmd, arg?) {
        let request = crate::builtins::convert::to_index(cmd)? as libc::c_ulong;
        let RubyValue::Int(fd) = fileno_value(recv)? else {
            return Err(io_error!("closed stream"));
        };
        let fd = fd as libc::c_int;
        let rc = match arg {
            Some(RubyValue::Str(s)) => {
                let mut buf = s.lock().bytes().to_vec();
                // SAFETY: `buf` is a live, writable allocation for the call.
                let rc = unsafe { libc::ioctl(fd, request, buf.as_mut_ptr()) };
                if rc >= 0 {
                    let mut g = s.lock();
                    let enc = g.encoding();
                    g.replace_bytes(buf, enc);
                }
                rc
            }
            Some(v) if !v.is_nil() => {
                let n = crate::builtins::convert::to_index(v)? as libc::c_int;
                // SAFETY: the by-value form passes an integer, not a pointer.
                unsafe { libc::ioctl(fd, request, n) }
            }
            // SAFETY: as above, with CRuby's default argument.
            _ => unsafe { libc::ioctl(fd, request, 0 as libc::c_int) },
        };
        if rc < 0 {
            let path = as_rio(recv).and_then(|io| io.path.lock().clone()).unwrap_or_default();
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(), "ioctl", &path));
        }
        Ok(RubyValue::Int(rc as i64))
    }

    // `#timeout`/`#timeout=` -- recorded and read back. CRuby raises
    // `IO::TimeoutError` when a blocking read outlives the value; zeo's reads
    // block, so nothing enforces it (documented in COMPATIBILITY.md). nil,
    // the default, means no timeout in CRuby either.
    def "timeout" (recv, &_blk) {
        Ok(match as_rio(recv) {
            Some(io) => io.timeout.lock().clone(),
            None => RubyValue::Nil,
        })
    }
    def "timeout=" (recv, seconds, &_blk) {
        if let Some(io) = as_rio(recv) {
            *io.timeout.lock() = seconds.clone();
        }
        Ok(seconds.clone())
    }

    // `#set_encoding_by_bom` -- if the stream STARTS with a byte-order mark,
    // consume it, make that the external encoding and answer it; otherwise
    // touch nothing and answer nil.
    def "set_encoding_by_bom" (recv, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Ok(RubyValue::Nil);
        };
        // ASCII-8BIT does NOT conflict: `rb_io_set_encoding_by_bom` REQUIRES
        // binmode, and binmode is exactly what sets the external encoding to
        // ASCII-8BIT. Only some other explicit encoding is the conflict.
        if io
            .encodings
            .lock()
            .0
            .is_some_and(|e| e != crate::encoding::ASCII_8BIT)
        {
            return Err(arg_error!("encoding is set to UTF-8 already"));
        }
        let Some((id, len)) = read_bom(recv)? else {
            return Ok(RubyValue::Nil);
        };
        // Only the BOM's own bytes are consumed; `with_file` left the
        // descriptor at the position Ruby believes in, so seeking forward by
        // the mark's length is what "skip it" means.
        with_file(recv, |f, path| {
            use std::io::Seek;
            f.seek(std::io::SeekFrom::Start(len as u64))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "seek", path))?;
            Ok(())
        })?;
        io.encodings.lock().0 = Some(id);
        Ok(crate::builtins::encoding::encoding_value(id))
    }

    def "flush" (recv, &_blk) {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        Ok(recv.clone())
    }

    def "fileno" | "to_i" (recv, &_blk) {
        fileno_value(recv)
    }

    def "tty?" | "isatty" (recv, &_blk) {
        use std::io::IsTerminal;
        Ok(RubyValue::Bool(match stream_of(recv) {
            Some(StdStream::Stdin) => std::io::stdin().is_terminal(),
            Some(StdStream::Stdout) => std::io::stdout().is_terminal(),
            Some(StdStream::Stderr) => std::io::stderr().is_terminal(),
            // A regular file is never a tty.
            None => false,
        }))
    }

    // `IO#winsize` (from `require "io/console"`) -- `[rows, columns]`, or
    // `Errno::ENOTTY` when the stream isn't a terminal, as CRuby answers. The
    // rest of the console surface is in `io_console.rs`; this row predates it.
    def "winsize" (recv, &_blk) {
        let fd = raw_fd(recv)?;
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } != 0 {
            return Err(super::io_console::not_a_terminal(recv, "IO#winsize"));
        }
        Ok(RubyValue::Array(crate::collections::array_new(vec![
            RubyValue::Int(ws.ws_row as i64),
            RubyValue::Int(ws.ws_col as i64),
        ])))
    }

    // `IO#nonblock?` -- the descriptor's own `O_NONBLOCK`, from
    // `require "io/nonblock"`.
    def "nonblock?" (recv, &_blk) {
        let Some(fd) = io_raw_fd(recv) else {
            return Ok(RubyValue::Bool(false));
        };
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(io_error!("closed stream"));
        }
        Ok(RubyValue::Bool(flags & libc::O_NONBLOCK != 0))
    }

    // `IO#nonblock(flag = true) { ... }` -- sets the flag for the block only and
    // restores it after, answering the block's value. CRuby REQUIRES the block
    // here (`#nonblock?` is the reader), so a blockless call is a LocalJumpError.
    def "nonblock" (recv, mode?, &blk) {
        let Some(RubyValue::Proc(p)) = blk else {
            return Err(crate::raise_error(
                "LocalJumpError",
                "no block given".to_string(),
            ));
        };
        let on = mode.is_none_or(|v| v.truthy());
        let Some(fd) = io_raw_fd(recv) else {
            return Err(io_error!("closed stream"));
        };
        let was = unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_NONBLOCK != 0;
        set_fd_nonblock(fd, on)?;
        let out = p.call(&[]);
        set_fd_nonblock(fd, was)?;
        out
    }

    // `IO#nonblock = flag` -- the plain setter, answering the flag it set.
    def "nonblock=" (recv, nonblock, &_blk) {
        let on = nonblock.truthy();
        let Some(fd) = io_raw_fd(recv) else {
            return Err(io_error!("closed stream"));
        };
        set_fd_nonblock(fd, on)?;
        Ok(RubyValue::Bool(on))
    }

    def "wait_readable" cfunc (recv, _timeout?, &_blk) {
        let args = __args;
        io_wait_for(recv, args, libc::POLLIN)
    }

    def "wait_writable" cfunc (recv, _timeout?, &_blk) {
        let args = __args;
        io_wait_for(recv, args, libc::POLLOUT)
    }

    // `wait_priority(timeout = nil)` -- out-of-band data only, which is why a
    // pipe carrying ordinary bytes answers nil. Goes through `select(2)`, not
    // `poll(2)`; see [`select_ready`].
    def "wait_priority" cfunc (recv, _timeout?, &_blk) {
        let args = __args;
        let (_, _, e) = select_ready(&[], &[], &[raw_fd(recv)?], wait_timeout_ms(args.first()))?;
        Ok(if e.first() == Some(&true) {
            recv.clone()
        } else {
            RubyValue::Nil
        })
    }

    // `wait(timeout = nil, *modes)` -- the three `wait_*` methods behind one
    // name. Each mode is a Symbol; several may be combined, and none means
    // `:read`.
    def "wait" (recv, *args, &_blk) {
        let mut events: libc::c_short = 0;
        for m in args.iter().skip(1) {
            let RubyValue::Symbol(s) = m else {
                return Err(arg_error!("unsupported mode: {}", m.inspect_string()));
            };
            events |= match s.name().as_str() {
                "read" | "readable" => libc::POLLIN,
                "write" | "writable" => libc::POLLOUT,
                "priority" => libc::POLLPRI,
                other => return Err(arg_error!("unsupported mode: {other}")),
            };
        }
        if events == 0 {
            events = libc::POLLIN;
        }
        io_wait_for(recv, &args[..args.len().min(1)], events)
    }

    // `IO#to_s` is NOT `#inspect`: CRuby leaves `to_s` as `Object`'s address
    // form (`#<IO:0x...>`, `#<File:0x...>`) even for `STDIN`, and only
    // `inspect` describes the stream. Interpolating an IO shows the address.
    def "inspect" (recv, &_blk) {
        let name = match stream_of(recv) {
            Some(StdStream::Stdin) => "#<IO:<STDIN>>".to_string(),
            Some(StdStream::Stdout) => "#<IO:<STDOUT>>".to_string(),
            Some(StdStream::Stderr) => "#<IO:<STDERR>>".to_string(),
            None => match as_rio(recv) {
                // A handle names its path when it has one, its descriptor when it
                // does not (a pipe end, a socket), and neither once it is closed.
                Some(io) => {
                    let class = crate::builtins::class_name_of(recv);
                    let closed = matches!(
                        &*io.backend.lock(),
                        IoBackend::File(None) | IoBackend::Pipe(None)
                    );
                    match (io.path.lock().as_deref(), closed) {
                        (Some(p), false) => format!("#<{class}:{p}>"),
                        (Some(p), true) => format!("#<{class}:{p} (closed)>"),
                        (None, false) => format!("#<{class}:fd {}>", raw_fd(recv)?),
                        (None, true) => format!("#<{class}:(closed)>"),
                    }
                }
                None => "#<IO>".to_string(),
            },
        };
        Ok(RubyValue::Str(crate::collections::string_new(name)))
    }

    def "sync" (recv, &_blk) {
        let on = as_rio(recv).is_some_and(|io| io.sync.load(std::sync::atomic::Ordering::Relaxed));
        Ok(RubyValue::Bool(on))
    }

    def "sync=" (recv, sync, &_blk) {
        let v = sync.clone();
        if let Some(io) = as_rio(recv) {
            io.sync
                .store(v.truthy(), std::sync::atomic::Ordering::Relaxed);
        }
        Ok(v)
    }

    // `path`/`to_path` -- the name this IO was opened from.
    def "path" | "to_path" (recv, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Err(io_error!("not a file"));
        };
        // A std stream reports its bracketed name -- unless a `#reopen` gave
        // it a real path, which CRuby then answers instead (`$stderr.reopen(
        // IO::NULL).path` is "/dev/null"). Anything else answers its path, or
        // nil when it has none (a pipe, or `File.new(fd)` with no `path:`).
        if let (IoBackend::Std(stream), None) = (&*io.backend.lock(), io.path.lock().as_ref()) {
            let name = match stream {
                StdStream::Stdin => "<STDIN>",
                StdStream::Stdout => "<STDOUT>",
                StdStream::Stderr => "<STDERR>",
            };
            return Ok(RubyValue::Str(crate::collections::string_new(
                name.to_string(),
            )));
        }
        Ok(match &*io.path.lock() {
            Some(p) => RubyValue::Str(crate::collections::string_new(p.clone())),
            None => RubyValue::Nil,
        })
    }

    // `read` / `read(n)` / `read(n, buf)` -- the whole rest, or `n` bytes,
    // optionally read INTO an existing String `buf` (returned in place of a fresh
    // one). At EOF, a LENGTHED read answers nil while a whole-rest read answers
    // `""` (real Ruby's asymmetry, and the thing a read loop tests).
    def "read" cfunc (recv, _length?, _outbuf?, &blk) {
        let args = __args;
        let result = io_read_val(recv, args, blk)?;
        // 2-arg `read(length, buffer)`: fill the caller's String and answer it (or
        // nil at EOF, having emptied it).
        if let Some(RubyValue::Str(buf)) = args.get(1) {
            match &result {
                RubyValue::Str(s) => {
                    let txt = s.lock().to_utf8_lossy().into_owned();
                    buf.lock().replace_utf8(txt);
                    return Ok(RubyValue::Str(buf.clone()));
                }
                RubyValue::Nil => {
                    buf.lock().replace_utf8(String::new());
                    return Ok(RubyValue::Nil);
                }
                _ => {}
            }
        }
        Ok(result)
    }

    // `gets([sep][, limit][, chomp:])` -- one line, or nil at EOF; bumps `lineno`.
    def "gets" cfunc (recv, _sep?, _limit?, **_opts, &_blk) {
        let args = __args;
        gets_value(recv, args)
    }

    // `readline` -- `gets`, but raises `EOFError` instead of answering nil.
    def "readline" params "sep = nil, limit = nil, chomp: nil" cfunc (recv, _sep?, _limit?, **_opts, &_blk) {
        let args = __args;
        match gets_value(recv, args)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            line => Ok(line),
        }
    }

    def "lineno" (recv, &_blk) {
        let n = as_rio(recv).map_or(0, |io| io.lineno.load(std::sync::atomic::Ordering::Relaxed));
        Ok(RubyValue::Int(n))
    }

    def "lineno=" (recv, lineno, &_blk) {
        let n = convert::to_index(lineno)?;
        if let Some(io) = as_rio(recv) {
            io.lineno.store(n, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(RubyValue::Int(n))
    }

    def "getc" (recv, &_blk) {
        getc_value(recv)
    }

    def "readchar" (recv, &_blk) {
        match getc_value(recv)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            ch => Ok(ch),
        }
    }

    def "getbyte" (recv, &_blk) {
        getbyte_value(recv)
    }

    def "readbyte" (recv, &_blk) {
        match getbyte_value(recv)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            b => Ok(b),
        }
    }

    // `readlines([sep][, limit][, chomp:])` -- every remaining line as an Array.
    // Drains through `gets`, so `$stdin` reads its own way (see `gets_value`)
    // instead of demanding a real file.
    def "readlines" cfunc (recv, _sep?, _limit?, **_opts, &_blk) {
        let args = __args;
        let mut lines = Vec::new();
        while let line @ (RubyValue::Str(_) | RubyValue::Object(_)) = gets_value(recv, args)? {
            lines.push(line);
        }
        Ok(RubyValue::Array(crate::collections::array_new(lines)))
    }

    // `each_line`/`each([sep][, limit][, chomp:])` -- yield each line; bumps
    // lineno. Drains through `gets` for the same reason `readlines` does.
    def "each_line" | "each" cfunc (recv, _sep?, _limit?, **_opts, &blk) {
        let args = __args;
        let p = crate::builtins::block_or_enum!(recv, args, blk);
        while let line @ (RubyValue::Str(_) | RubyValue::Object(_)) = gets_value(recv, args)? {
            p.call(&[line])?;
        }
        Ok(recv.clone())
    }

    // `each_char` -- yield each UTF-8 char.
    def "each_char" (recv, &blk) {
        let p = crate::builtins::block_or_enum!(recv, __args, blk);
        loop {
            let ch = with_buffered_file(recv, |io, f, path| {
                read_one_char(io, f)
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "each_char", path))
            })?;
            match ch {
                Some(s) => p.call(&[RubyValue::Str(crate::collections::string_new(s))])?,
                None => break,
            };
        }
        Ok(recv.clone())
    }

    // `each_byte` -- yield each byte as an Integer.
    def "each_byte" (recv, &blk) {
        let p = crate::builtins::block_or_enum!(recv, __args, blk);
        let text = io_read_val(recv, &[], None)?;
        let RubyValue::Str(s) = &text else {
            return Ok(recv.clone());
        };
        let bytes = s.lock().bytes().to_vec();
        for b in bytes {
            p.call(&[RubyValue::Int(b as i64)])?;
        }
        Ok(recv.clone())
    }

    // `printf(fmt, *args)` -- format and write, answering nil.
    def "printf" cfunc (recv, fmt, *args, &_blk) {
        let s = crate::builtins::format::sprintf(&fmt.try_display_string()?, args)?;
        write_str(recv_io(recv)?, &s)?;
        Ok(RubyValue::Nil)
    }

    // `putc(int | str)` -- write one character (an Integer's low byte, or a
    // String's first character), answering the argument unchanged.
    def "putc" (recv, char, &_blk) {
        let bytes = putc_bytes(char)?;
        write_bytes(recv_io(recv)?, &bytes)?;
        Ok(char.clone())
    }

    // `readpartial(maxlen)` / `sysread(maxlen)` -- read up to `maxlen` bytes,
    // blocking for at least one; `EOFError` at EOF (unlike `read(n)`'s nil).
    def "readpartial" | "sysread" cfunc (recv, _maxlen, _outbuf?, &_blk) {
        let args = __args;
        let RubyValue::Int(max) = args.first().cloned().unwrap_or(RubyValue::Nil) else {
            return Err(arg_error!("length must be an Integer"));
        };
        let outbuf = match args.get(1) {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(crate::builtins::convert::to_rstr(v)?),
        };
        let bytes = with_file(recv, |f, path| {
            let mut buf = vec![0u8; max.max(0) as usize];
            let got = blocking_read(f, &mut buf)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "read", path))?;
            buf.truncate(got);
            Ok(buf)
        })?;
        if bytes.is_empty() && max > 0 {
            // CRuby empties the buffer before raising, so a rescued EOF leaves no
            // stale bytes from the previous read.
            if let Some(buf) = &outbuf {
                buf.lock().replace_utf8(String::new());
            }
            return Err(eof_error!("end of file reached"));
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // The second argument is an output BUFFER: CRuby fills it in place and
        // returns that same object, so the caller may read the bytes back out of
        // it or compare with `equal?`.
        match outbuf {
            Some(buf) => {
                buf.lock().replace_utf8(text);
                Ok(args[1].clone())
            }
            None => Ok(RubyValue::Str(crate::collections::string_new(text))),
        }
    }

    // `read_nonblock(maxlen, outbuf = nil, exception: true)` -- one `read(2)` that
    // never waits. The descriptor is marked `O_NONBLOCK` first and LEFT that way,
    // as CRuby leaves it; every blocking row here already parks in `poll(2)` on
    // `EAGAIN`, so an ordinary `#gets` on the same handle still blocks.
    def "read_nonblock" params "len, buf = nil, exception: nil" (recv, maxlen, buffer?, **opts, &_blk) {
        let raises = nonblock_raises(opts);
        let max = convert::to_index(maxlen)?.max(0) as usize;
        let outbuf = match buffer {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(convert::to_rstr(v)?),
        };
        set_fd_nonblock(raw_fd(recv)?, true)?;
        let read = with_file(recv, |f, _path| {
            let mut buf = vec![0u8; max];
            loop {
                match std::io::Read::read(f, &mut buf) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Ok(n) => {
                        buf.truncate(n);
                        return Ok(Ok(buf));
                    }
                    Err(e) => return Ok(Err(e)),
                }
            }
        })?;
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return would_block(false, raises, "read");
            }
            Err(e) => return Err(crate::builtins::file::raise_errno(&e, "read", "")),
        };
        if bytes.is_empty() && max > 0 {
            // CRuby empties the buffer before reporting EOF, so a rescued end
            // leaves no stale bytes from the previous read.
            if let Some(buf) = &outbuf {
                buf.lock().replace_utf8(String::new());
            }
            return if raises {
                Err(eof_error!("end of file reached"))
            } else {
                Ok(RubyValue::Nil)
            };
        }
        // TAG the bytes rather than decoding them: a byte count can land
        // mid-character, so binary is the only honest answer (as `#read(n)`).
        match outbuf {
            Some(buf) => {
                buf.lock().replace_bytes(bytes, crate::encoding::ASCII_8BIT);
                Ok(buffer.expect("outbuf is Some only when a buffer was passed").clone())
            }
            None => Ok(RubyValue::Str(crate::string_from_bytes(
                bytes,
                crate::encoding::ASCII_8BIT,
            ))),
        }
    }

    // `write_nonblock(string, exception: true)` -- one `write(2)` that never
    // waits, answering the count it managed. A partial write is the caller's to
    // resume, which is the whole point of the method.
    def "write_nonblock" params "buf, exception: nil" (recv, buffer, **opts, &_blk) {
        let raises = nonblock_raises(opts);
        let bytes = convert::to_rstr(buffer)?.lock().bytes().to_vec();
        set_fd_nonblock(raw_fd(recv)?, true)?;
        let wrote = with_file(recv, |f, _path| {
            loop {
                match std::io::Write::write(f, &bytes) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    other => return Ok(other),
                }
            }
        })?;
        match wrote {
            Ok(n) => Ok(RubyValue::Int(n as i64)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => would_block(true, raises, "write"),
            Err(e) => Err(crate::builtins::file::raise_errno(&e, "write", "")),
        }
    }

    // `seek(offset, whence = IO::SEEK_SET)` -- answers 0, like real Ruby.
    def "seek" cfunc (recv, _offset, _whence?, &_blk) {
        let args = __args;
        let off = offset_of(args.first().unwrap_or(&RubyValue::Nil))?;
        let whence = match args.get(1) {
            None => 0,
            Some(w) => whence_of(w)?,
        };
        with_file(recv, |f, path| {
            use std::io::Seek;
            let pos = match whence {
                0 => std::io::SeekFrom::Start(off.max(0) as u64),
                1 => std::io::SeekFrom::Current(off),
                2 => std::io::SeekFrom::End(off),
                _ => return Err(arg_error!("invalid whence")),
            };
            f.seek(pos)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "seek", path))?;
            Ok(RubyValue::Int(0))
        })
    }

    // `sysseek(offset, whence = SEEK_SET)` -- seek, answering the new absolute
    // position (unlike `seek`, which answers 0).
    def "sysseek" cfunc (recv, _offset, _whence?, &_blk) {
        let args = __args;
        let off = offset_of(args.first().unwrap_or(&RubyValue::Nil))?;
        let whence = match args.get(1) {
            None => 0,
            Some(w) => whence_of(w)?,
        };
        with_file(recv, |f, path| {
            use std::io::Seek;
            let pos = match whence {
                0 => std::io::SeekFrom::Start(off.max(0) as u64),
                1 => std::io::SeekFrom::Current(off),
                2 => std::io::SeekFrom::End(off),
                _ => return Err(arg_error!("invalid whence")),
            };
            let p = f
                .seek(pos)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "sysseek", path))?;
            Ok(RubyValue::Int(p as i64))
        })
    }

    def "tell" | "pos" (recv, &_blk) {
        with_file(recv, |f, path| {
            use std::io::Seek;
            let p = f
                .stream_position()
                .map_err(|e| crate::builtins::file::raise_errno(&e, "tell", path))?;
            Ok(RubyValue::Int(p as i64))
        })
    }

    // `pos=` -- seek to an absolute byte offset.
    def "pos=" (recv, _pos, &_blk) {
        let n = offset_of(__args.first().unwrap_or(&RubyValue::Nil))?;
        with_file(recv, |f, path| {
            use std::io::Seek;
            f.seek(std::io::SeekFrom::Start(n.max(0) as u64))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "pos=", path))?;
            Ok(RubyValue::Int(n))
        })
    }

    def "rewind" (recv, &_blk) {
        with_file(recv, |f, path| {
            use std::io::Seek;
            f.rewind()
                .map_err(|e| crate::builtins::file::raise_errno(&e, "rewind", path))?;
            Ok(RubyValue::Int(0))
        })
    }

    def "eof?" | "eof" (recv, &_blk) {
        // stdin can't seek; peek the shared buffered reader instead. `fill_buf`
        // is non-destructive -- an empty buffer means end-of-input.
        if matches!(stream_of(recv), Some(StdStream::Stdin)) {
            use std::io::BufRead;
            // `fill_buf` on an empty buffer BLOCKS for the next chunk of input.
            let empty = crate::gvl::without_gvl(|| {
                std::io::stdin()
                    .lock()
                    .fill_buf()
                    .map(|b| b.is_empty())
                    .unwrap_or(true)
            });
            return Ok(RubyValue::Bool(empty));
        }
        with_file(recv, |f, path| {
            use std::io::Seek;
            let pos = f
                .stream_position()
                .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
            let end = f
                .seek(std::io::SeekFrom::End(0))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
            // Peeking at the end MOVES the position -- put it back.
            f.seek(std::io::SeekFrom::Start(pos))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "eof?", path))?;
            Ok(RubyValue::Bool(pos >= end))
        })
    }

    // `#stat` -- an `fstat(2)` snapshot of the open descriptor as a `File::Stat`.
    def "stat" (recv, &_blk) {
        stat_value(recv)
    }

    // `#fcntl(cmd[, arg])` -- the raw `fcntl(2)`; answers its integer result
    // (e.g. `fcntl(F_GETFD)` reads the close-on-exec flag). `arg` defaults to 0.
    def "fcntl" cfunc (recv, _cmd, _arg?, &_blk) {
        let cmd = int_of(&__args[0])? as libc::c_int;
        let arg = match __args.get(1) {
            Some(v) => int_of(v)? as libc::c_int,
            None => 0,
        };
        use std::os::fd::AsRawFd;
        with_file(recv, |f, path| {
            // SAFETY: `f` owns a valid fd for the call's duration.
            let r = unsafe { libc::fcntl(f.as_raw_fd(), cmd, arg) };
            if r < 0 {
                return Err(crate::builtins::file::raise_errno(
                    &std::io::Error::last_os_error(),
                    "fcntl",
                    path,
                ));
            }
            Ok(RubyValue::Int(r as i64))
        })
    }

    // `pid` -- the child an `IO.popen` handle is connected to, `nil` for every
    // other IO (CRuby's split exactly).
    def "pid" (recv, &_blk) {
        match as_rio(recv).map(|io| io.child_pid.load(std::sync::atomic::Ordering::Relaxed)) {
            Some(pid) if pid != 0 => Ok(RubyValue::Int(pid)),
            _ => Ok(RubyValue::Nil),
        }
    }

    // `#fsync`/`#fdatasync` -- flush to disk; answers 0.
    def "fsync" | "fdatasync" (recv, &_blk) {
        with_file(recv, |f, path| {
            f.sync_all()
                .map_err(|e| crate::builtins::file::raise_errno(&e, "fsync", path))?;
            Ok(RubyValue::Int(0))
        })
    }

    // `close` -- idempotent (a second close is a no-op, as in Ruby), and it
    // DROPS the descriptor, so every later operation raises IOError. Closing an
    // `IO.popen` handle also waits for the child and sets `$?`, CRuby's contract
    // (the fd must drop FIRST -- a `w`-mode child only exits on stdin's EOF).
    def "close" (recv, &_blk) {
        close_io(recv)
    }

    // `#close_read` -- close the readable half. On a write-only stream CRuby
    // raises IOError; otherwise the stream is closed. Answers nil.
    def "close_read" (recv, &_blk) {
        if fd_access_mode(recv) == Some(libc::O_WRONLY) {
            return Err(io_error!("not opened for reading"));
        }
        if let Some(io) = as_rio(recv) {
            let mut b = io.backend.lock();
            if let IoBackend::Pipe(Some(f)) = &*b {
                use std::os::fd::AsRawFd;
                // The read-direction mirror of `close_write` above.
                // SAFETY: the fd is open, borrowed from the live backend.
                if unsafe { libc::shutdown(f.as_raw_fd(), libc::SHUT_RD) } == 0 {
                    return Ok(RubyValue::Nil);
                }
            }
            // The same close(2) `#close` performs, so it reports the same way.
            let closed = b.close_file();
            drop(b);
            closed.map_err(|e| crate::builtins::file::raise_bare_errno(&e))?;
        }
        Ok(RubyValue::Nil)
    }

    // `#close_write` -- close the writable half. On a read-only stream there is
    // none, so CRuby raises IOError; otherwise the stream is closed. Answers nil.
    def "close_write" (recv, &_blk) {
        if fd_access_mode(recv) == Some(libc::O_RDONLY) {
            return Err(io_error!("not opened for writing"));
        }
        if let Some(io) = as_rio(recv) {
            let mut b = io.backend.lock();
            if let IoBackend::Pipe(Some(f)) = &*b {
                use std::os::fd::AsRawFd;
                // A socket half-closes: `shutdown(2)` the write direction and keep
                // reading -- what `Socket#close_write` means, and what lets a
                // duplex `IO.popen` handle deliver EOF to the child while the
                // parent still reads its answer. ENOTSOCK (an ordinary pipe fd)
                // falls through to the full close.
                // SAFETY: the fd is open, borrowed from the live backend.
                if unsafe { libc::shutdown(f.as_raw_fd(), libc::SHUT_WR) } == 0 {
                    return Ok(RubyValue::Nil);
                }
            }
            // The same close(2) `#close` performs, so it reports the same way.
            let closed = b.close_file();
            drop(b);
            closed.map_err(|e| crate::builtins::file::raise_bare_errno(&e))?;
        }
        Ok(RubyValue::Nil)
    }

    def "closed?" (recv, &_blk) {
        let closed = match as_rio(recv) {
            Some(io) => matches!(
                &*io.backend.lock(),
                IoBackend::File(None) | IoBackend::Pipe(None)
            ),
            None => false,
        };
        Ok(RubyValue::Bool(closed))
    }

    // `#binmode` -- binary mode. The newline half is a no-op on Unix, but the
    // ENCODING half is not: CRuby's `rb_io_binmode` sets the external
    // encoding to ASCII-8BIT, so everything the stream hands back is bytes.
    def "binmode" (recv, &_blk) {
        if let Some(io) = as_rio(recv) {
            io.binmode.store(true, Relaxed);
            let mut encs = io.encodings.lock();
            encs.0 = Some(crate::encoding::ASCII_8BIT);
            encs.1 = None;
        }
        Ok(recv.clone())
    }

    def "binmode?" (recv, &_blk) {
        Ok(RubyValue::Bool(
            as_rio(recv).is_some_and(|io| io.binmode.load(Relaxed)),
        ))
    }

    // `#external_encoding` -- the encoding this stream's bytes are read as.
    // Unset, a READABLE stream answers `Encoding.default_external`; a write-only
    // one answers nil, since nothing is being decoded.
    def "external_encoding" (recv, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Ok(RubyValue::Nil);
        };
        if let Some(id) = io.encodings.lock().0 {
            return Ok(crate::builtins::encoding::encoding_value(id));
        }
        // CRuby's `rb_io_external_encoding` asks WRITABLE, not write-only:
        // a stream opened `"w+"` answers nil too, because nothing has said
        // what its bytes are to be read as.
        let writable = matches!(stream_of(recv), Some(StdStream::Stdout | StdStream::Stderr))
            || matches!(fd_access_mode(recv), Some(libc::O_WRONLY | libc::O_RDWR));
        Ok(if writable {
            RubyValue::Nil
        } else {
            crate::builtins::encoding::encoding_value(crate::encoding::default_external())
        })
    }

    // `#internal_encoding` -- what reads are transcoded TO. nil unless asked for,
    // which is the default for every stream.
    def "internal_encoding" (recv, &_blk) {
        Ok(match as_rio(recv).and_then(|io| io.encodings.lock().1) {
            Some(id) => crate::builtins::encoding::encoding_value(id),
            None => RubyValue::Nil,
        })
    }

    // `#set_encoding(ext[, int])` -- record the pair and answer the receiver. A
    // single `"EXT:INT"` string names both. zeo's IO reads bytes and tags them,
    // so this is what the tag comes from; no transcoding happens on the way in.
    def "set_encoding" cfunc (recv, _external, _internal?, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Ok(recv.clone());
        };
        let parse = |v: &RubyValue| -> Result<Option<crate::encoding::EncodingId>, Signal> {
            match v {
                RubyValue::Nil => Ok(None),
                other => Ok(Some(crate::builtins::encoding::arg_encoding(other)?)),
            }
        };
        // The combined `"UTF-8:BINARY"` spelling, which only a String can carry.
        if __args.len() == 1
            && let RubyValue::Str(sp) = &__args[0] {
                let spec = sp.lock().to_utf8_lossy().into_owned();
                if let Some((ext, int)) = spec.split_once(':') {
                    let ext = crate::builtins::encoding::arg_encoding(&RubyValue::Str(
                        crate::string_new(ext.to_string()),
                    ))?;
                    let int = crate::builtins::encoding::arg_encoding(&RubyValue::Str(
                        crate::string_new(int.to_string()),
                    ))?;
                    *io.encodings.lock() = (Some(ext), Some(int));
                    return Ok(recv.clone());
                }
            }
        let ext = parse(&__args[0])?;
        let int = match __args.get(1) {
            Some(v) => parse(v)?,
            None => None,
        };
        *io.encodings.lock() = (ext, int);
        Ok(recv.clone())
    }

    // `#autoclose = flag` -- answers the assigned value (Ruby setter convention).
    def "autoclose=" (recv, autoclose, &_blk) {
        set_autoclose(recv, autoclose);
        Ok(autoclose.clone())
    }

    def "autoclose?" (recv, &_blk) {
        Ok(RubyValue::Bool(
            as_rio(recv).is_none_or(|io| io.autoclose.load(Relaxed)),
        ))
    }

    // `#to_io` -- an IO answers itself.
    def "to_io" (recv, &_blk) {
        Ok(recv.clone())
    }

    // `#close_on_exec?` -- CRuby marks a newly-opened fd close-on-exec by default
    // (since Ruby 2.0), so this reports true; `#close_on_exec=` records the wish
    // and answers it (the flag has no observable effect without an exec here).
    def "close_on_exec?" (recv, &_blk) {
        let _ = recv;
        Ok(RubyValue::Bool(true))
    }

    def "close_on_exec=" (_recv, _close_on_exec, &_blk) {
        Ok(__args[0].clone())
    }

    // `#advise(kind[, offset, len])` -- a hint to the kernel about access
    // patterns. Validated against the known symbols, then a no-op answering nil
    // (`posix_fadvise` is best-effort and unobservable from Ruby).
    def "advise" cfunc (recv, _advice, _offset?, _len?, &_blk) {
        // Not a conversion site: CRuby's io_advise requires a bare Symbol
        // ("advice must be a Symbol", oracle-verified).
        let kind = match &__args[0] {
            RubyValue::Symbol(s) => s.name().to_string(),
            _ => return Err(type_error!("advice must be a Symbol")),
        };
        if !matches!(
            kind.as_str(),
            "normal" | "sequential" | "random" | "willneed" | "dontneed" | "noreuse"
        ) {
            return Err(not_impl_error!("Unsupported advice: :{kind}"));
        }
        let _ = recv;
        Ok(RubyValue::Nil)
    }

    // `#ungetbyte(int_or_str)` -- push bytes back so the next read returns them
    // first. Recorded on the IO's unget stack (see `getbyte_value`).
    def "ungetbyte" | "ungetc" (recv, _byte, &_blk) {
        let Some(io) = as_rio(recv) else {
            return Err(io_error!("not a file"));
        };
        let mut ug = io.unget.lock();
        match &__args[0] {
            RubyValue::Int(i) => ug.push((*i & 0xff) as u8),
            // Push in reverse so the string's bytes read back in order (the stack
            // is LIFO).
            RubyValue::Str(s) => {
                for b in s
                    .lock()
                    .to_utf8_lossy()
                    .into_owned()
                    .into_bytes()
                    .into_iter()
                    .rev()
                {
                    ug.push(b);
                }
            }
            RubyValue::Nil => {}
            other => {
                let s = convert::to_rstr(other)?;
                for b in s
                    .lock()
                    .to_utf8_lossy()
                    .into_owned()
                    .into_bytes()
                    .into_iter()
                    .rev()
                {
                    ug.push(b);
                }
            }
        }
        Ok(RubyValue::Nil)
    }

    // `#pread(maxlen, offset[, buffer])` -- read at a fixed offset WITHOUT moving
    // the position (`pread(2)`). Answers a new String, or fills `buffer` when
    // given and answers it. EOFError when nothing is available at `offset`.
    def "pread" cfunc (recv, _maxlen, _offset, _buffer?, &_blk) {
        let count = int_of(&__args[0])?.max(0) as usize;
        let offset = offset_of(&__args[1])?.max(0) as u64;
        let data = with_file(recv, |f, path| {
            use std::os::unix::fs::FileExt;
            let mut buf = vec![0u8; count];
            let n = f
                .read_at(&mut buf, offset)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "pread", path))?;
            if n == 0 && count > 0 {
                return Err(eof_error!("end of file reached"));
            }
            buf.truncate(n);
            Ok(buf)
        })?;
        let text = String::from_utf8_lossy(&data).into_owned();
        match __args.get(2) {
            Some(RubyValue::Str(buf)) => {
                buf.lock().replace_utf8(text);
                Ok(RubyValue::Str(buf.clone()))
            }
            _ => Ok(RubyValue::Str(crate::collections::string_new(text))),
        }
    }

    // `#pwrite(string, offset)` -- write at a fixed offset WITHOUT moving the
    // position; answers the number of bytes written.
    def "pwrite" (recv, _buffer, _offset, &_blk) {
        let bytes = match &__args[0] {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned().into_bytes(),
            other => other.try_display_string()?.into_bytes(),
        };
        let offset = offset_of(&__args[1])?.max(0) as u64;
        with_file(recv, |f, path| {
            use std::os::unix::fs::FileExt;
            let n = f
                .write_at(&bytes, offset)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "pwrite", path))?;
            Ok(RubyValue::Int(n as i64))
        })
    }

    // `#reopen(other_io_or_path[, mode])` -- `rb_io_reopen`: the receiver KEEPS
    // its descriptor NUMBER and starts referring to the target, which is the
    // whole point (`$stderr.reopen(path)` redirects fd 2, so a child process
    // and a C-level write follow it too). So: open the target, `dup2` it onto
    // the receiver's fd, and drop the temporary.
    //
    // Opening the target FIRST is what the mode argument is for -- `"w"`
    // implies `O_CREAT|O_TRUNC`, so a missing path is created rather than
    // ENOENT. Answers self.
    def "reopen" cfunc (recv, _target, _mode?, &_blk) {
        use std::os::fd::AsRawFd;
        let Some(io) = as_rio(recv) else {
            return Ok(recv.clone());
        };
        // The IO-to-IO form duplicates the OTHER IO's live descriptor; the
        // path form opens one, honouring the mode.
        match as_rio(&__args[0]) {
            Some(other) => {
                let path = other.path.lock().clone();
                let fd = other
                    .raw_fd()
                    .ok_or_else(|| crate::dispatch::raise_error(
                        "IOError", "closed stream".to_string()))?;
                io.dup2_from(fd, path)?;
            }
            None => {
                let path = crate::builtins::file::path_arg(&__args[0], "reopen")?;
                // With no mode of its own the call INHERITS the receiver's --
                // `File.open(x, "w").reopen(y)` writes `y`, where a default
                // of "r" would answer EBADF on the first write.
                let inherited = io.open_mode.lock().clone().map(|m| {
                    RubyValue::Str(crate::collections::string_new(m))
                });
                let mode = __args.get(1).or(inherited.as_ref());
                let f = crate::builtins::file::open_options_for(mode, None)?
                    .open(&path)
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "reopen", &path))?;
                io.dup2_from(f.as_raw_fd(), Some(path))?;
            }
        }
        Ok(recv.clone())
    }

    // `#each_codepoint { |cp| ... }` -- yield each remaining character's codepoint;
    // answers self.
    def "each_codepoint" (recv, &blk) {
        let p = crate::builtins::block_or_enum!(recv, __args, blk);
        let content = with_file(recv, |f, path| {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match blocking_read(f, &mut chunk)
                    .map_err(|e| crate::builtins::file::raise_errno(&e, "each_codepoint", path))?
                {
                    0 => break,
                    k => buf.extend_from_slice(&chunk[..k]),
                }
            }
            Ok(buf)
        })?;
        for ch in String::from_utf8_lossy(&content).chars() {
            p.call(&[RubyValue::Int(ch as i64)])?;
        }
        Ok(recv.clone())
    }


    // `IO.pipe` -- a `[reader, writer]` pair over a `pipe(2)`; each end is a plain
    // `IO`. With a block, yields the pair and closes both ends afterward.
    def self."pipe" (recv, _external?, _internal?, &block) {
        use std::os::fd::FromRawFd;
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `fds` is a 2-element array `pipe(2)` fills with the read/write fds.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(crate::builtins::file::raise_errno(
                &std::io::Error::last_os_error(),
                "pipe",
                "",
            ));
        }
        // Both ends start non-blocking, as CRuby's do -- `IO.pipe` there hands back
        // fds it has already marked, which `#nonblock?` reports -- and
        // close-on-exec, also as CRuby's (see `set_fd_cloexec`).
        for fd in fds {
            set_fd_nonblock(fd, true)?;
            set_fd_cloexec(fd);
        }
        // SAFETY: `pipe(2)` just handed us these two fresh, owned fds.
        let r = pipe_value(unsafe { std::fs::File::from_raw_fd(fds[0]) });
        let w = pipe_value(unsafe { std::fs::File::from_raw_fd(fds[1]) });
        // CRuby marks the WRITE end unbuffered, and only that end.
        if let Some(io) = as_rio(&w) {
            io.sync.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let pair = RubyValue::Array(crate::collections::array_new(vec![r.clone(), w.clone()]));
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(pair);
        };
        let out = p.call(&[pair]);
        let _ = close_io(&r);
        let _ = close_io(&w);
        let _ = recv; // `IO.pipe`'s receiver is unused
        out
    }

    // `IO.popen([env,] cmd, mode = "r" [, opts])` -- spawn `cmd` with the far end
    // of a pipe as its stdout (`"r"`), its stdin (`"w"`), or both (`"r+"`/`"w+"`),
    // answering the near end as an IO that knows its child: `#pid` answers the
    // child's, and `#close` reaps it into `$?`. `cmd` is a shell String or a
    // direct argv Array; env and options hashes ride through the spawn builder.
    // A duplex mode uses a `socketpair(2)` so ONE descriptor serves both
    // directions (the historical popen trick), keeping the handle on the
    // ordinary pipe backend. With a block, yields the IO and closes it after.
    def self."popen" cfunc (_recv, _command, _mode?, _opt?, _extra?, &block) {
        use std::os::fd::FromRawFd;
        use std::process::Stdio;

        let mut rest = __args;
        let mut spawn_args: Vec<RubyValue> = Vec::new();
        if let Some(env @ RubyValue::Hash(_)) = rest.first() {
            spawn_args.push(env.clone());
            rest = &rest[1..];
        }
        let cmd_arg = rest.first().ok_or_else(|| arg_error!("no command given"))?;
        rest = &rest[1..];
        match cmd_arg {
            // `IO.popen("-")` forks the interpreter itself -- there is no second
            // interpreter image to run in an AOT-compiled program.
            RubyValue::Str(s) if s.lock().to_utf8_lossy() == "-" => {
                return Err(not_impl_error!(
                    "IO.popen(\"-\") (fork) is not supported by zeo"
                ));
            }
            RubyValue::Array(a) => spawn_args.extend(a.lock().iter().cloned()),
            other => spawn_args.push(other.clone()),
        }
        let mode = match rest.first() {
            Some(RubyValue::Str(m)) => {
                rest = &rest[1..];
                m.lock().to_utf8_lossy().into_owned()
            }
            _ => "r".to_string(),
        };
        if let Some(opts @ RubyValue::Hash(_)) = rest.first() {
            spawn_args.push(opts.clone());
        }
        // "r+", "rb:UTF-8", ... -- only the direction matters on Unix.
        let mode = mode.split(':').next().unwrap_or("r");
        let duplex = mode.contains('+');
        let write = mode.starts_with('w') || mode.starts_with('a');

        let mut cmd = crate::builtins::process::build_spawn_command(&spawn_args)?;
        let io = if duplex {
            let mut fds = [0 as libc::c_int; 2];
            // SAFETY: `fds` is a 2-element array `socketpair(2)` fills.
            if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) } != 0 {
                return Err(crate::builtins::file::raise_errno(
                    &std::io::Error::last_os_error(),
                    "popen",
                    "",
                ));
            }
            // Close-on-exec like every fd this runtime creates; the child's copies
            // are re-opened by the exec-time `dup2` (see `set_fd_cloexec`).
            for fd in fds {
                set_fd_cloexec(fd);
            }
            // SAFETY: `socketpair(2)` just handed us these two fresh, owned fds.
            let parent = unsafe { std::fs::File::from_raw_fd(fds[0]) };
            // SAFETY: as above -- the child end, handed to the Command.
            let child_end = unsafe { std::fs::File::from_raw_fd(fds[1]) };
            cmd.stdin(Stdio::from(
                child_end
                    .try_clone()
                    .map_err(|e| crate::builtins::process::spawn_error(&e))?,
            ));
            cmd.stdout(Stdio::from(child_end));
            let child = cmd
                .spawn()
                .map_err(|e| crate::builtins::process::spawn_error(&e))?;
            popen_value(parent, i64::from(child.id()))
        } else {
            if write {
                cmd.stdin(Stdio::piped());
            } else {
                cmd.stdout(Stdio::piped());
            }
            let mut child = cmd
                .spawn()
                .map_err(|e| crate::builtins::process::spawn_error(&e))?;
            let f: std::fs::File = if write {
                std::os::fd::OwnedFd::from(child.stdin.take().expect("stdin was piped")).into()
            } else {
                std::os::fd::OwnedFd::from(child.stdout.take().expect("stdout was piped")).into()
            };
            popen_value(f, i64::from(child.id()))
        };
        // `Command` keeps the Stdio fds it was handed until it drops -- for the
        // duplex socketpair that is the child's own end, and holding it here would
        // deny the parent its EOF forever. The block form reads below, so drop NOW.
        drop(cmd);

        let Some(RubyValue::Proc(p)) = block else {
            return Ok(io);
        };
        let out = p.call(std::slice::from_ref(&io));
        // The close both drops the fd and reaps the child into `$?`, so the block
        // form leaves `$?` set even when the block never read to EOF.
        let _ = close_io(&io);
        out
    }

    // `IO.copy_stream(src, dst)` -- copy `src` to `dst`, answering the byte count.
    // Each end is either an IO-like object (read from / written to at its current
    // position, via `read`/`write`) or a filename (String/`to_path`).
    def self."copy_stream" cfunc (_recv, _src, _dst, _copy_length?, _src_offset?, &_blk) {
        let bytes = if is_io_object(&__args[0]) {
            match crate::dispatch::send_value(&__args[0], crate::Symbol::intern("read"), &[], None)? {
                RubyValue::Str(s) => s.lock().bytes().to_vec(),
                RubyValue::Nil => Vec::new(), // EOF
                other => crate::builtins::convert::to_rstr(&other)?
                    .lock()
                    .bytes()
                    .to_vec(),
            }
        } else {
            let src = crate::builtins::file::path_arg(&__args[0], "copy_stream")?;
            crate::gvl::without_gvl(|| std::fs::read(&src))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", &src))?
        };
        let n = bytes.len();

        if is_io_object(&__args[1]) {
            let s = RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT));
            crate::dispatch::send_value(&__args[1], crate::Symbol::intern("write"), &[s], None)?;
        } else {
            let dst = crate::builtins::file::path_arg(&__args[1], "copy_stream")?;
            crate::gvl::without_gvl(|| std::fs::write(&dst, &bytes))
                .map_err(|e| crate::builtins::file::raise_errno(&e, "copy_stream", &dst))?;
        }
        Ok(RubyValue::Int(n as i64))
    }

    // `IO.sysopen(path, mode = "r", perm = 0o666)` -- open and answer the raw fd
    // Integer (the caller owns closing it). Same flag rules as `File.open`,
    // through the same helper: this row used to open READ-ONLY whatever it was
    // asked for, so an `IO.new(fd, "w")` over the result raised EBADF on the
    // first write.
    def self."sysopen" cfunc (_recv, _path, _mode?, _perm?, &_blk) {
        use std::os::fd::IntoRawFd;
        let path = crate::builtins::file::path_arg(&__args[0], "sysopen")?;
        let opts = crate::builtins::file::open_options_for(__args.get(1), __args.get(2))?;
        // Gvl-released for the reason `File.open` releases it: open(2) blocks
        // on a FIFO with no peer.
        let f = crate::gvl::without_gvl(|| opts.open(&path))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "rb_sysopen", &path))?;
        Ok(RubyValue::Int(f.into_raw_fd() as i64))
    }

    // `IO.select(read, write, except, timeout = nil)` -- which of the given
    // handles are ready, as `[readable, writable, exceptional]`, or nil if the
    // timeout expires first. One `poll(2)` over every listed descriptor; a handle
    // may appear in more than one list and is polled once per appearance, so the
    // answer keeps each list's own order.
    def self."select" (_recv, _read?, _write?, _error?, _timeout?, &_blk) {
        let list = |i: usize| -> Result<Vec<RubyValue>, Signal> {
            match __args.get(i) {
                None | Some(RubyValue::Nil) => Ok(Vec::new()),
                Some(v) => Ok(crate::builtins::convert::to_rary(v)?.lock().to_vec()),
            }
        };
        let sets = [
            (list(0)?, libc::POLLIN),
            (list(1)?, libc::POLLOUT),
            (list(2)?, libc::POLLPRI),
        ];
        let timeout_ms = wait_timeout_ms(__args.get(3));

        if sets.iter().all(|(ios, _)| ios.is_empty()) {
            // Nothing to watch: CRuby still honours the timeout, then answers nil.
            if timeout_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
            }
            return Ok(RubyValue::Nil);
        }
        let mut fds: [Vec<libc::c_int>; 3] = Default::default();
        for (i, (ios, _)) in sets.iter().enumerate() {
            for io in ios {
                fds[i].push(raw_fd(io)?);
            }
        }
        let (r, w, e) = select_ready(&fds[0], &fds[1], &fds[2], timeout_ms)?;
        let mut any = false;
        let mut out = Vec::new();
        for ((ios, _), flags) in sets.iter().zip([r, w, e]) {
            let ready: Vec<RubyValue> = ios
                .iter()
                .zip(flags)
                .filter(|(_, ok)| *ok)
                .map(|(io, _)| io.clone())
                .collect();
            any |= !ready.is_empty();
            out.push(RubyValue::Array(crate::collections::array_new(ready)));
        }
        if !any {
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(crate::collections::array_new(out)))
    }

    // `IO.try_convert(obj)`: `obj` if it is already an IO, its `to_io` if it
    // defines one, else nil.
    def self."try_convert" (_recv, _object, &_blk) {
        if as_rio(&__args[0]).is_some() {
            return Ok(__args[0].clone());
        }
        let sym = crate::Symbol::intern("to_io");
        if !crate::dispatch::responds_to_value(&__args[0], sym, true) {
            return Ok(RubyValue::Nil);
        }
        let answer = crate::dispatch::send_value(&__args[0], sym, &[], None)?;
        if answer.is_nil() || as_rio(&answer).is_some() {
            return Ok(answer);
        }
        Err(crate::builtins::type_error!(
            "can't convert {0} to IO ({0}#to_io gives {1})",
            crate::builtins::convert_name_of(&__args[0]),
            crate::builtins::class_name_of(&answer)
        ))
    }

    // `IO.new(fd)` / `IO.open(fd)` -- wrap an existing descriptor. `IO.for_fd` is
    // the same. The fd is adopted (closing the IO closes it) unless
    // `autoclose: false` says otherwise.
    def self."new" | "open" | "for_fd" cfunc (_recv, _fd, _mode?, &block) {
        use std::os::fd::FromRawFd;
        let fd = &convert::to_index(&__args[0])?;
        // An fd that names no open descriptor is CRuby's `Errno::EBADF`, raised
        // here rather than left to fail on the first read.
        if unsafe { libc::fcntl(*fd as libc::c_int, libc::F_GETFD) } < 0 {
            return Err(crate::dispatch::raise_error(
                "Errno::EBADF",
                "Bad file descriptor".to_string(),
            ));
        }
        // SAFETY: the fd was just confirmed open, and the caller vouches it is
        // theirs to adopt.
        let io = pipe_value(unsafe { std::fs::File::from_raw_fd(*fd as libc::c_int) });
        if let Some(RubyValue::Hash(opts)) = __args.get(1) {
            let key = RubyValue::Symbol(crate::Symbol::intern("autoclose"));
            if !crate::collections::hash_get(opts, &key).truthy() {
                set_autoclose(&io, &RubyValue::Bool(false));
            }
        }
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(io);
        };
        let out = p.call(std::slice::from_ref(&io));
        let _ = close_io(&io);
        out
    }

    // Re-init rebinds this handle to `fd` (+ an optional mode string and
    // opts), the same descriptor adoption `IO.new` performs, and the
    // per-handle state (lineno, pushed-back bytes, read-ahead, encodings)
    // starts over. The previous descriptor is RELEASED, not closed -- re-init
    // closes nothing -- and the File-vs-pipe shape is kept so `#class` stays
    // what it was.
    private def "initialize" cfunc (recv, *args, &_block) {
        use std::os::fd::FromRawFd;
        crate::builtins::check_arity(args.len(), 1, Some(3))?;
        let Some(io) = as_rio(recv) else {
            return Err(crate::builtins::type_error!("not an IO"));
        };
        let fd = convert::to_index(&args[0])?;
        if unsafe { libc::fcntl(fd as libc::c_int, libc::F_GETFD) } < 0 {
            return Err(crate::dispatch::raise_error(
                "Errno::EBADF",
                "Bad file descriptor".to_string(),
            ));
        }
        // SAFETY: the fd was just confirmed open, and re-init adopts it.
        let f = unsafe { std::fs::File::from_raw_fd(fd as libc::c_int) };
        {
            let mut b = io.backend.lock();
            let was_file = matches!(&*b, IoBackend::File(_));
            b.release_file();
            *b = if was_file {
                IoBackend::File(Some(f))
            } else {
                IoBackend::Pipe(Some(f))
            };
        }
        reset_handle_state(io);
        if let Some(RubyValue::Hash(opts)) = args.last() {
            let key = RubyValue::Symbol(crate::Symbol::intern("autoclose"));
            let v = crate::collections::hash_get(opts, &key);
            if !v.is_nil() && !v.truthy() {
                set_autoclose(recv, &RubyValue::Bool(false));
            }
        }
        Ok(recv.clone())
    }

    // `#initialize_copy` -- adopt a `dup(2)` of the other handle's descriptor
    // (the two share a file position, as CRuby's `IO#dup` pair does). A
    // std-stream source stays a std-stream handle: zeo's std streams are
    // positionless globals, not descriptors to duplicate.
    private def "initialize_copy"(recv, other) {
        use std::os::fd::{AsRawFd, FromRawFd};
        let Some(io) = as_rio(recv) else {
            return Err(crate::builtins::type_error!("not an IO"));
        };
        let Some(src) = as_rio(other) else {
            return Err(crate::builtins::type_error!(
                "initialize_copy should take same class object"
            ));
        };
        let dup_slot = |slot: &Option<std::fs::File>| -> Result<Option<std::fs::File>, Signal> {
            let Some(f) = slot else {
                return Err(crate::builtins::io_error!("closed stream"));
            };
            let fd = unsafe { libc::dup(f.as_raw_fd()) };
            if fd < 0 {
                return Err(crate::builtins::file::raise_errno(
                    &std::io::Error::last_os_error(),
                    "dup",
                    "",
                ));
            }
            // SAFETY: `dup(2)` just handed us this descriptor to own.
            Ok(Some(unsafe { std::fs::File::from_raw_fd(fd) }))
        };
        let new_backend = match &*src.backend.lock() {
            IoBackend::Std(s) => IoBackend::Std(*s),
            IoBackend::File(slot) => IoBackend::File(dup_slot(slot)?),
            IoBackend::Pipe(slot) => IoBackend::Pipe(dup_slot(slot)?),
        };
        {
            let mut b = io.backend.lock();
            b.release_file();
            *b = new_backend;
        }
        reset_handle_state(io);
        io.lineno.store(
            src.lineno.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        io.binmode.store(
            src.binmode.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        io.sync.store(
            src.sync.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        *io.encodings.lock() = *src.encodings.lock();
        Ok(recv.clone())
    }

    // `io/console`'s additions to IO. The rows are declared here, with the rest
    // of IO's surface, because one class owns one table; the termios work they
    // stand on earns its own file. Unconditional -- the `require` is ceremony.

    def "winsize=" (recv, _size) {
        super::io_console::winsize_set(recv, __args, __block)
    }

    def "raw" (recv, *_args) {
        super::io_console::raw(recv, __args, __block)
    }

    def "raw!" (recv, *_args) {
        super::io_console::raw_bang(recv, __args, __block)
    }

    def "cooked" (recv) {
        super::io_console::cooked(recv, __args, __block)
    }

    def "cooked!" (recv) {
        super::io_console::cooked_bang(recv, __args, __block)
    }

    def "echo?" (recv) {
        super::io_console::echo_p(recv, __args, __block)
    }

    def "echo=" (recv, _echo) {
        super::io_console::echo_set(recv, __args, __block)
    }

    def "noecho" (recv) {
        super::io_console::noecho(recv, __args, __block)
    }

    def "getch" (recv, *_args) {
        super::io_console::getch(recv, __args, __block)
    }

    def "getpass" (recv, *_args) {
        super::io_console::getpass(recv, __args, __block)
    }

    def "iflush" (recv) {
        super::io_console::iflush(recv, __args, __block)
    }

    def "oflush" (recv) {
        super::io_console::oflush(recv, __args, __block)
    }

    def "ioflush" (recv) {
        super::io_console::ioflush(recv, __args, __block)
    }

    def "ttyname" (recv) {
        super::io_console::ttyname(recv, __args, __block)
    }

    def "console_mode" (recv) {
        super::io_console::console_mode(recv, __args, __block)
    }

    def "console_mode=" (recv, _mode) {
        super::io_console::console_mode_set(recv, __args, __block)
    }

    def "pressed?" (recv) {
        super::io_console::pressed_p(recv, __args, __block)
    }

    def "check_winsize_changed" (recv) {
        super::io_console::check_winsize_changed(recv, __args, __block)
    }

    def "beep" (recv) {
        super::io_console::beep(recv, __args, __block)
    }

    def "clear_screen" (recv) {
        super::io_console::clear_screen(recv, __args, __block)
    }

    def "erase_line" (recv, _mode) {
        super::io_console::erase_line(recv, __args, __block)
    }

    def "erase_screen" (recv, _mode) {
        super::io_console::erase_screen(recv, __args, __block)
    }

    def "goto" (recv, _line, _column) {
        super::io_console::goto(recv, __args, __block)
    }

    def "goto_column" (recv, _column) {
        super::io_console::goto_column(recv, __args, __block)
    }

    def "cursor" (recv) {
        super::io_console::cursor(recv, __args, __block)
    }

    def "cursor=" (recv, _position) {
        super::io_console::cursor_set(recv, __args, __block)
    }

    def "cursor_up" (recv, _n) {
        super::io_console::cursor_up(recv, __args, __block)
    }

    def "cursor_down" (recv, _n) {
        super::io_console::cursor_down(recv, __args, __block)
    }

    def "cursor_left" (recv, _n) {
        super::io_console::cursor_left(recv, __args, __block)
    }

    def "cursor_right" (recv, _n) {
        super::io_console::cursor_right(recv, __args, __block)
    }

    def "scroll_forward" (recv, _n) {
        super::io_console::scroll_forward(recv, __args, __block)
    }

    def "scroll_backward" (recv, _n) {
        super::io_console::scroll_backward(recv, __args, __block)
    }


    // `IO.console` -- the controlling terminal, from `io/console`.
    def self."console" cfunc (recv) {
        super::io_console::io_class_console(recv, __args, __block)
    }

    // The whole-file family is identical to `File`'s -- run File's own rows so
    // the two class methods can never drift, in shape or in behavior.

    def self."read" cfunc (recv, _path, _length?, _offset?, _opt?) {
        file_class_row("read", recv, __args, __block)
    }

    def self."write" cfunc (recv, _path, _data, _offset?) {
        file_class_row("write", recv, __args, __block)
    }

    def self."binread" cfunc (recv, _path, _length?, _offset?) {
        file_class_row("binread", recv, __args, __block)
    }

    def self."binwrite" cfunc (recv, _path, _data, _offset?) {
        file_class_row("binwrite", recv, __args, __block)
    }

    def self."readlines" cfunc (recv, _path, _sep?, _opt?) {
        file_class_row("readlines", recv, __args, __block)
    }

    def self."foreach" cfunc (recv, _path, _sep?, _opt?) {
        file_class_row("foreach", recv, __args, __block)
    }
}

/// The `IO::SEEK_*` and `IO::READABLE`/`WRITABLE`/`PRIORITY` constants --
/// seeded from generated `main()` beside the stdio ones. The open/lock flags
/// are NOT here: those are `File::Constants`, which `IO` includes.
pub fn seed_io_constants() {
    let io = zeo_abi::IO_CLASS.0;
    crate::constants::const_set(io, "SEEK_SET", RubyValue::Int(0));
    crate::constants::const_set(io, "SEEK_CUR", RubyValue::Int(1));
    crate::constants::const_set(io, "SEEK_END", RubyValue::Int(2));
    // Sparse-file seeks. Darwin has no `SEEK_HOLE`/`SEEK_DATA` in libc, so the
    // values are CRuby's own (`io.c` defines them unconditionally).
    crate::constants::const_set(io, "SEEK_HOLE", RubyValue::Int(3));
    crate::constants::const_set(io, "SEEK_DATA", RubyValue::Int(4));
    // The `IO#wait` event mask.
    crate::constants::const_set(io, "READABLE", RubyValue::Int(1));
    crate::constants::const_set(io, "PRIORITY", RubyValue::Int(2));
    crate::constants::const_set(io, "WRITABLE", RubyValue::Int(4));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(v.to_string()))
    }

    /// A BINARY-tagged string holding raw bytes -- what `Integer#chr`
    /// (128..=255), `String#b`, and binary IO reads produce.
    fn bin(bytes: &[u8]) -> RubyValue {
        RubyValue::Str(crate::collections::string_from_bytes(
            bytes.to_vec(),
            crate::encoding::ASCII_8BIT,
        ))
    }

    // --- display_bytes: the print-family accumulator ------------------

    #[test]
    fn display_bytes_keeps_a_binary_strings_raw_bytes() {
        // THE regression this seam exists for: `0xB4` must stay one byte,
        // not the Latin-1 -> UTF-8 promotion `0xC2 0xB4` that corrupted
        // bm_ao_render/bm_so_mandelbrot's image output.
        let mut buf = Vec::new();
        display_bytes(&bin(&[0xb4]), &mut buf).unwrap();
        assert_eq!(buf, [0xb4]);
    }

    #[test]
    fn display_bytes_renders_utf8_strings_and_non_strings_as_display_text() {
        let mut buf = Vec::new();
        display_bytes(&s("héllo"), &mut buf).unwrap();
        display_bytes(&RubyValue::Int(42), &mut buf).unwrap();
        display_bytes(&RubyValue::Nil, &mut buf).unwrap(); // `print nil` -> ""
        assert_eq!(buf, "héllo42".as_bytes());
    }

    #[test]
    fn display_bytes_keeps_every_byte_of_a_longer_binary_string() {
        let mut buf = Vec::new();
        display_bytes(&bin(&[0x00, 0x7f, 0x80, 0xff]), &mut buf).unwrap();
        assert_eq!(buf, [0x00, 0x7f, 0x80, 0xff]);
    }

    // --- render_puts: CRuby's exact line shapes, now byte-faithful ----

    #[test]
    fn render_puts_writes_a_bare_newline_for_no_args_but_nothing_for_empty_arrays() {
        let mut buf = Vec::new();
        render_puts(&[], &mut buf).unwrap();
        assert_eq!(buf, b"\n");
        buf.clear();
        render_puts(&[RubyValue::Array(crate::array_new(Vec::new()))], &mut buf).unwrap();
        assert_eq!(buf, b"");
    }

    #[test]
    fn render_puts_adds_one_newline_and_never_doubles_a_trailing_one() {
        let mut buf = Vec::new();
        render_puts(&[s("a"), s("b\n")], &mut buf).unwrap();
        assert_eq!(buf, b"a\nb\n");
    }

    #[test]
    fn render_puts_flattens_nested_arrays_recursively() {
        let inner = RubyValue::Array(crate::array_new(vec![s("b"), s("c")]));
        let outer = RubyValue::Array(crate::array_new(vec![s("a"), inner]));
        let mut buf = Vec::new();
        render_puts(&[outer], &mut buf).unwrap();
        assert_eq!(buf, b"a\nb\nc\n");
    }

    #[test]
    fn render_puts_preserves_binary_bytes_and_still_terminates_the_line() {
        let mut buf = Vec::new();
        render_puts(&[bin(&[0xb4])], &mut buf).unwrap();
        assert_eq!(buf, [0xb4, b'\n']);
        // A binary string ENDING in 0x0A already has its line ending.
        buf.clear();
        render_puts(&[bin(&[0xb4, b'\n'])], &mut buf).unwrap();
        assert_eq!(buf, [0xb4, b'\n']);
    }

    #[test]
    fn render_puts_marks_a_self_referential_array_instead_of_recursing() {
        let arr = crate::array_new(vec![s("a")]);
        arr.lock().push(RubyValue::Array(arr.clone()));
        let mut buf = Vec::new();
        render_puts(&[RubyValue::Array(arr)], &mut buf).unwrap();
        assert_eq!(buf, b"a\n[...]\n");
    }

    // --- putc_bytes: one character, in the argument's own encoding ----

    #[test]
    fn putc_bytes_takes_an_integers_low_byte() {
        assert_eq!(putc_bytes(&RubyValue::Int(0xb4)).unwrap(), [0xb4]);
        assert_eq!(putc_bytes(&RubyValue::Int(0x1234)).unwrap(), [0x34]);
        // NUM2CHR truncates a Float (oracle-verified).
        assert_eq!(putc_bytes(&RubyValue::Float(65.9)).unwrap(), [65]);
    }

    #[test]
    fn putc_bytes_takes_a_strings_first_character_in_its_own_encoding() {
        // UTF-8: the first CHARACTER (multibyte stays whole).
        assert_eq!(putc_bytes(&s("ab")).unwrap(), b"a");
        assert_eq!(putc_bytes(&s("éx")).unwrap(), "é".as_bytes());
        // BINARY: exactly one raw byte, no UTF-8 promotion.
        assert_eq!(putc_bytes(&bin(&[0xb4, 0x01])).unwrap(), [0xb4]);
        assert_eq!(putc_bytes(&s("")).unwrap(), Vec::<u8>::new());
    }

    #[test]
    #[should_panic(expected = "no implicit conversion from nil to integer")]
    fn putc_bytes_rejects_a_non_character_argument() {
        // NUM2CHR raises CRuby's TypeError; with no registry installed the
        // unit context surfaces it through `raise_error`'s panic fallback,
        // message intact.
        let _ = putc_bytes(&RubyValue::Nil);
    }

    // --- write_value / write_bytes: the fd-facing seam ----------------

    /// A File-backed `RIo` over a fresh temp file, plus its path for
    /// reading the bytes back.
    fn temp_file_io(tag: &str) -> (RubyValue, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("zeo_rt_io_test_{tag}_{}", std::process::id()));
        let f = std::fs::File::create(&path).expect("temp file");
        (file_value(f, Some(path.display().to_string())), path)
    }

    #[test]
    fn write_value_sends_a_binary_strings_raw_bytes_and_counts_them() {
        let (io, path) = temp_file_io("binary");
        let n = write_value(&io, &bin(&[0x00, 0xb4, 0xff])).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read(&path).unwrap(), [0x00, 0xb4, 0xff]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn write_value_counts_a_utf8_strings_bytes_and_renders_non_strings() {
        let (io, path) = temp_file_io("mixed");
        assert_eq!(write_value(&io, &s("é")).unwrap(), 2);
        assert_eq!(write_value(&io, &RubyValue::Int(42)).unwrap(), 2);
        assert_eq!(std::fs::read(&path).unwrap(), "é42".as_bytes());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn write_bytes_reaches_the_backend_untouched() {
        let (io, path) = temp_file_io("bytes");
        write_bytes(&io, &[0xc2, 0xb4, 0x00]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [0xc2, 0xb4, 0x00]);
        let _ = std::fs::remove_file(path);
    }
}
