//! Opening and closing: what a mode string and an options Hash say, how a
//! descriptor becomes a handle, and what the two non-blocking answers are.

use super::*;

/// Everything `IO.new`'s MODE argument and options Hash say about the handle
/// it is about to wrap.
///
/// Read BEFORE the descriptor is adopted, so an access mode that names nothing
/// or an encoding name that does not exist raises without leaving an fd owned
/// by a half-built handle.
pub(super) struct OpenOpts {
    /// A `b` in the mode, or `binmode: true`.
    pub(super) binary: bool,
    /// Whether anything NAMED an encoding -- what decides whether `b` gets to
    /// claim the external slot for ASCII-8BIT.
    pub(super) named_enc: bool,
    pub(super) ext: crate::encoding::EncodingId,
    pub(super) int: Option<crate::encoding::EncodingId>,
    /// `path:`, which names the handle for `#path` without opening one.
    pub(super) path: Option<String>,
    pub(super) autoclose: bool,
    /// The `(read, write)` access the mode asks for, when one was named at
    /// all. `None` means "whatever the descriptor already is", which is what
    /// makes a bare `IO.new(fd)` legal on any open fd.
    pub(super) access: Option<(bool, bool)>,
}

/// Read [`OpenOpts`] out of `IO.new`'s two optional arguments. The options are
/// the LAST Hash of the two, since `IO.new(fd, **opts)` puts them where the
/// mode would otherwise sit.
pub(super) fn open_opts(
    mode: Option<&RubyValue>,
    opts: Option<&RubyValue>,
) -> Result<OpenOpts, Signal> {
    use crate::builtins::file;
    // `**opts` has already peeled the keyword Hash, so the mode slot holds
    // only a real mode.
    let trailing = opts;
    // The mode slot takes EITHER spelling from an object: `to_str` for a mode
    // string, `to_int` for an `O_*` bitmask. Ruby tries the string first
    // (`rb_io_extract_modeenc`), so a class offering both is read as one.
    // Normalized up front, which leaves every rule below reading a real Str or
    // Int -- and only a value with neither still reaches the TypeError.
    let converted = mode.and_then(file::int_mode).transpose()?;
    let mode = converted.as_ref().or(mode);
    // An Integer mode is an `O_*` bitmask and names no encoding; anything else
    // must convert to a String, which is where `IO.new(fd, Object.new)` gets
    // its TypeError -- and a BRACED Hash too, since the keyword peel above has
    // already taken any Hash the caller actually meant as options.
    let mode_str = match mode {
        Some(RubyValue::Str(s)) => Some(s.lock().to_utf8_lossy().into_owned()),
        Some(RubyValue::Int(_)) => None,
        None | Some(RubyValue::Nil) => file::kwarg_str(trailing, "mode"),
        Some(v) => match crate::builtins::convert::to_str(v)? {
            RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
            _ => None,
        },
    };
    // Nothing is opened here, but the access mode is still checked: CRuby
    // refuses `IO.new(fd, "zz")` at the call rather than on the first read.
    if let Some(m) = mode_str.as_deref() {
        file::open_options(m)?;
    }
    let mode_enc = mode_str.as_deref().and_then(|m| file::split_mode(m).1);
    let (ext, int) = file::read_encodings_with(trailing, mode_enc)?;
    Ok(OpenOpts {
        binary: mode_str
            .as_deref()
            .is_some_and(|m| file::split_mode(m).0.contains('b'))
            || file::kwarg(trailing, "binmode").is_some_and(|v| v.truthy()),
        named_enc: mode_enc.is_some()
            || ["encoding", "external_encoding", "internal_encoding"]
                .iter()
                .any(|k| file::kwarg(trailing, k).is_some()),
        ext,
        int,
        path: file::kwarg_str(trailing, "path"),
        autoclose: file::kwarg(trailing, "autoclose").is_none_or(|v| v.truthy()),
        access: match mode {
            Some(RubyValue::Int(flags)) => Some(match *flags & libc::O_ACCMODE as i64 {
                x if x == libc::O_WRONLY as i64 => (false, true),
                x if x == libc::O_RDWR as i64 => (true, true),
                _ => (true, false),
            }),
            _ => mode_str.as_deref().map(|m| {
                let base = file::split_mode(m).0;
                let plus = base.contains('+');
                match base.chars().next() {
                    Some('w') | Some('a') => (plus, true),
                    _ => (true, plus),
                }
            }),
        },
    })
}

/// CRuby refuses a mode the descriptor cannot serve (`IO.new(read_only_fd,
/// "w")` is `Errno::EINVAL`, not a write that fails later): the read/write
/// bits the mode asks for must be a subset of the ones `open(2)` gave the fd.
/// A handle given no mode of its own inherits the descriptor's, so it is
/// always legal.
pub(super) fn check_fd_access(fd: i64, o: &OpenOpts) -> Result<(), Signal> {
    let Some((want_r, want_w)) = o.access else {
        return Ok(());
    };
    let flags = unsafe { libc::fcntl(fd as libc::c_int, libc::F_GETFL) };
    if flags < 0 {
        return Ok(());
    }
    let acc = flags & libc::O_ACCMODE;
    let has_r = acc == libc::O_RDONLY || acc == libc::O_RDWR;
    let has_w = acc == libc::O_WRONLY || acc == libc::O_RDWR;
    match (want_r && !has_r) || (want_w && !has_w) {
        true => Err(crate::dispatch::raise_error(
            "Errno::EINVAL",
            "Invalid argument".to_string(),
        )),
        false => Ok(()),
    }
}

/// `klass.new`'s body for the whole open family: the RECEIVER decides what the
/// first argument means, because CRuby reaches `rb_file_initialize` for a File
/// and `rb_io_initialize` for an IO. Their parameter lists differ, so the arity
/// range does too -- one method reporting `1..3` to File and `1..2` to IO.
pub(super) fn new_handle(
    recv: &RubyValue,
    args: &[RubyValue],
    opts: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    // `ancestors_of_value`, not the frozen chain: a class born at run time
    // (`Class.new(File)`) has no compile-time entry at all, and asking the
    // frozen registry answers "not a File" for one.
    let file_side = matches!(recv, RubyValue::Class(c)
        if crate::dispatch::ancestors_of_value(*c).contains(&zeo_abi::FILE_CLASS));
    let max = match file_side {
        true => 3,
        false => 2,
    };
    crate::builtins::check_arity(args.len(), 1, Some(max))?;
    let io = match file_side {
        true => crate::builtins::file::open_path(&args[0], args.get(1), args.get(2), opts)?,
        false => io_from_fd(&args[0], args.get(1), opts)?,
    };
    tag_receiver_class(&io, recv);
    Ok(io)
}

/// Give a freshly built handle the RECEIVER's class, so `Class.new(File).open`
/// answers an instance of itself and `File.for_fd` answers a File over a
/// descriptor the backend would otherwise call a plain IO. A receiver that
/// already matches what the backend implies is left alone.
pub(super) fn tag_receiver_class(io: &RubyValue, recv: &RubyValue) {
    let RubyValue::Class(cid) = recv else { return };
    if let RubyValue::Object(o) = io
        && let Some(h) = o.as_any().downcast_ref::<RIo>()
        && h.class_id() != *cid
    {
        h.class_override
            .store(cid.0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// `IO.new`'s half of the shared open row -- CRuby's `rb_io_initialize`, which
/// an IO receiver reaches where a File receiver reaches `rb_file_initialize`.
///
/// The descriptor is ADOPTED (closing the handle closes it) unless
/// `autoclose: false` says otherwise.
pub(super) fn io_from_fd(
    fd: &RubyValue,
    mode: Option<&RubyValue>,
    opts: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    use std::os::fd::FromRawFd;
    let fd = crate::builtins::convert::to_index(fd)?;
    // An fd that names no open descriptor is CRuby's `Errno::EBADF`, raised
    // here rather than left to fail on the first read.
    if unsafe { libc::fcntl(fd as libc::c_int, libc::F_GETFD) } < 0 {
        return Err(crate::dispatch::raise_error(
            "Errno::EBADF",
            "Bad file descriptor".to_string(),
        ));
    }
    // Both arguments are read BEFORE the fd is adopted: a bad access mode or a
    // bad encoding name must raise without a handle owning the descriptor.
    let o = open_opts(mode, opts)?;
    check_fd_access(fd, &o)?;
    // SAFETY: the fd was just confirmed open, and the caller vouches it is
    // theirs to adopt.
    let f = unsafe { std::fs::File::from_raw_fd(fd as libc::c_int) };
    let io = RubyValue::Object(Arc::new(RIo::new(IoBackend::Pipe(Some(f)), o.path.clone())));
    apply_open_opts(&io, &o)?;
    Ok(io)
}

/// Put [`OpenOpts`] onto a handle that has just adopted its descriptor.
pub(super) fn apply_open_opts(io: &RubyValue, o: &OpenOpts) -> Result<(), Signal> {
    if o.binary {
        crate::dispatch::send_value(io, crate::Symbol::intern("binmode"), &[], None)?;
    }
    // `b` claims the external slot only when nothing NAMED an encoding: CRuby
    // answers UTF-8 for `"rb:UTF-8"` and still reports `#binmode?`.
    let binary_only = o.binary && !o.named_enc;
    if binary_only || o.named_enc || o.ext != crate::encoding::default_external() || o.int.is_some()
    {
        let ext = match binary_only {
            true => crate::encoding::ASCII_8BIT,
            false => o.ext,
        };
        set_handle_encodings(io, Some(ext), o.int);
    }
    if !o.autoclose {
        set_autoclose(io, &RubyValue::Bool(false));
    }
    Ok(())
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
        IoBackend::Pipe(None) | IoBackend::File(None) | IoBackend::Uninit => return None,
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
pub(super) fn fd_access_mode(recv: &RubyValue) -> Option<libc::c_int> {
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

/// `#close`'s work, also run when `IO.pipe`/`IO.popen` close a handle they
/// own after a block.
pub(super) fn close_io(recv: &RubyValue) -> Result<RubyValue, Signal> {
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
pub(super) fn set_autoclose(recv: &RubyValue, on: &RubyValue) {
    if let Some(io) = as_rio(recv) {
        io.autoclose.store(on.truthy(), Relaxed);
    }
}
