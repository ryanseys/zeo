//! IO, files, paths and processes.
//!
//! Every entry here is a Ruby method call. zeo's `IO`, `File`, `Dir` and
//! `Process` are real classes over real syscalls, so `rb_io_puts` is
//! `IO#puts` and cannot drift from it -- and the `$stdout` a program
//! reassigned is the one a C extension writes to, which a direct `write(2)`
//! would not be.
//!
//! # The descriptor entries are the exception
//!
//! `rb_cloexec_open` and its four neighbours are raw file descriptors, not
//! Ruby objects: MRI has them because it must set `FD_CLOEXEC` atomically so
//! a concurrent `fork` cannot leak the descriptor. That reasoning holds here
//! too, so they are real syscalls rather than method calls -- and each uses
//! the atomic form (`O_CLOEXEC`, `dup3`, `F_DUPFD_CLOEXEC`) where the
//! platform has one.
//!
//! `rb_wait_for_single_fd`, the two `rb_io_wait_*` and `rb_fdopen` are the
//! same shape: a bare `int` and no IO in sight. The two `wait` entries read
//! `errno` to decide, which is the whole reason they take a descriptor rather
//! than the object -- the caller has already had a read fail.
//!
//! Every one of them polls in a loop around `check_ints`, so a thread parked
//! in a C extension's wait still answers `Thread#kill`.

use super::convert::{to_value, value_of};
use super::layout as mri;
use super::object::{args_of, cstr, send};
use super::value::Value;
use crate::builtins::wrong_arg_type;
use crate::{RubyValue, Signal, Symbol};
use std::ffi::{c_char, c_int, c_long};

fn class_named(name: &str) -> Result<RubyValue, Signal> {
    crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, name)
        .ok_or_else(|| crate::builtins::name_error!("uninitialized constant {name}"))
}

fn a_string(text: &str) -> RubyValue {
    crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, text)
}

/// The last `errno`, as the raise zeo's own IO layer makes.
fn last_errno() -> Signal {
    crate::builtins::file::raise_bare_errno(&std::io::Error::last_os_error())
}

crate::cext_fn! {
    // ---- writing ---------------------------------------------------------

    fn rb_io_puts(argc: c_int, argv: *const Value, io: Value) -> Value {
        io_send(io, "puts", argc, argv)
    }

    fn rb_io_print(argc: c_int, argv: *const Value, io: Value) -> Value {
        io_send(io, "print", argc, argv)
    }

    fn rb_io_printf(argc: c_int, argv: *const Value, io: Value) -> Value {
        io_send(io, "printf", argc, argv)
    }

    /// `rb_io_addstr(io, str)`: `IO#<<`, which answers the IO so writes
    /// chain.
    fn rb_io_addstr(io: Value, s: Value) -> Value {
        let target = unsafe { value_of(io) };
        let text = unsafe { value_of(s) };
        send(&target, "<<", &[text])?;
        Ok(io)
    }

    /// `rb_p(v)`: `Kernel#p`, which prints `inspect` and a newline.
    fn rb_p(v: Value) -> () {
        let val = unsafe { value_of(v) };
        crate::dispatch::send_value(
            &crate::dispatch::main_object(),
            Symbol::intern("p"),
            &[val],
            None,
        )?;
        Ok(())
    }

    /// `rb_write_error(msg)`: straight to `$stderr`, which is where MRI's
    /// own goes -- so a redirected `$stderr` catches it.
    fn rb_write_error(msg: *const c_char) -> () {
        write_error(unsafe { super::string::borrow_bytes(msg, -1) })
    }

    fn rb_write_error2(msg: *const c_char, len: isize) -> () {
        write_error(unsafe { super::string::borrow_bytes(msg, len as c_long) })
    }

    fn rb_gets() -> Value {
        to_value(&crate::dispatch::send_value(
            &crate::dispatch::main_object(),
            Symbol::intern("gets"),
            &[],
            None,
        )?)
    }

    // ---- opening ---------------------------------------------------------

    fn rb_file_open(path: *const c_char, mode: *const c_char) -> Value {
        open_file(a_string(&unsafe { cstr(path) }), &unsafe { cstr(mode) })
    }

    fn rb_file_open_str(path: Value, mode: *const c_char) -> Value {
        open_file(unsafe { value_of(path) }, &unsafe { cstr(mode) })
    }

    /// `rb_io_fdopen(fd, flags, path)`: an IO over an existing descriptor.
    /// `path` is only the name for error messages, which is why it can be
    /// null.
    fn rb_io_fdopen(fd: c_int, _flags: c_int, _path: *const c_char) -> Value {
        let io = class_named("IO")?;
        to_value(&send(&io, "new", &[RubyValue::Int(fd as i64)])?)
    }

    /// `rb_io_ascii8bit_binmode(io)`: binary mode AND ASCII-8BIT, which is
    /// what `IO#binmode` already does in zeo.
    fn rb_io_ascii8bit_binmode(io: Value) -> Value {
        let target = unsafe { value_of(io) };
        send(&target, "binmode", &[])?;
        Ok(io)
    }

    // ---- paths -----------------------------------------------------------

    /// `rb_get_path(v)`: a String, or the `to_path` answer. An embedded NUL
    /// is refused, because the path is about to reach a syscall.
    fn rb_get_path(v: Value) -> Value {
        let val = unsafe { value_of(v) };
        to_value(&a_string(&crate::builtins::file::path_arg(&val, "to_path")?))
    }

    fn rb_get_path_no_checksafe(v: Value) -> Value {
        unsafe { Ok(rb_get_path(v)) }
    }

    fn rb_file_expand_path(path: Value, dir: Value) -> Value {
        file_class_call("expand_path", &[path, dir])
    }

    fn rb_file_absolute_path(path: Value, dir: Value) -> Value {
        file_class_call("absolute_path", &[path, dir])
    }

    fn rb_file_s_expand_path(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&send(&class_named("File")?, "expand_path", &args)?)
    }

    fn rb_file_s_absolute_path(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&send(&class_named("File")?, "absolute_path", &args)?)
    }

    fn rb_file_dirname(path: Value) -> Value {
        let p = unsafe { value_of(path) };
        to_value(&send(&class_named("File")?, "dirname", &[p])?)
    }

    /// `rb_file_directory_p(_, path)`: the first argument is MRI's unused
    /// receiver slot, kept so the prototype matches.
    fn rb_file_directory_p(_recv: Value, path: Value) -> Value {
        let p = unsafe { value_of(path) };
        to_value(&send(&class_named("File")?, "directory?", &[p])?)
    }

    fn rb_file_size(io: Value) -> i64 {
        let target = unsafe { value_of(io) };
        match send(&target, "size", &[])? {
            RubyValue::Int(n) => Ok(n),
            other => Err(wrong_arg_type(&other, "Integer")),
        }
    }

    fn rb_dir_getwd() -> Value {
        to_value(&send(&class_named("Dir")?, "pwd", &[])?)
    }

    /// `rb_find_file(path)`: the file `require` would load for this name,
    /// searched down `$LOAD_PATH`. Answers nil when nothing matches.
    fn rb_find_file(path: Value) -> Value {
        let p = unsafe { value_of(path) };
        let name = crate::builtins::file::path_arg(&p, "to_path")?;
        to_value(&match crate::features::resolve_on_disk(&name, 0, true) {
            Some(found) => a_string(&found.to_string_lossy()),
            None => RubyValue::Nil,
        })
    }

    /// `rb_find_file_ext(&path, exts)`: the same, trying each extension in
    /// a NULL-terminated list. Rewrites `*path` and answers the 1-based
    /// index of the extension that matched, or 0.
    fn rb_find_file_ext(path: *mut Value, exts: *const *const c_char) -> c_int {
        if path.is_null() || exts.is_null() {
            return Ok(0);
        }
        // SAFETY: the caller's own `VALUE` slot.
        let base = unsafe { value_of(path.read()) };
        let stem = crate::builtins::file::path_arg(&base, "to_path")?;
        let mut i = 0;
        loop {
            // SAFETY: the caller promised a NULL-terminated list.
            let ext = unsafe { exts.offset(i).read() };
            if ext.is_null() {
                return Ok(0);
            }
            let name = format!("{stem}{}", unsafe { cstr(ext) });
            if let Some(found) = crate::features::resolve_on_disk(&name, 0, false) {
                let v = to_value(&a_string(&found.to_string_lossy()))?;
                unsafe { path.write(v) };
                return Ok(i as c_int + 1);
            }
            i += 1;
        }
    }

    // ---- process ---------------------------------------------------------

    fn rb_last_status_get() -> Value {
        to_value(&crate::globals::global_get(0, "$?"))
    }

    fn rb_last_status_set(status: c_int, pid: i32) -> () {
        let cls = class_named("Process")?;
        let RubyValue::Class(cid) = cls else {
            return Ok(());
        };
        let Some(st) = crate::constants::const_get(cid.0, "Status") else {
            return Ok(());
        };
        let built = send(
            &st,
            "new",
            &[RubyValue::Int(i64::from(pid)), RubyValue::Int(i64::from(status))],
        );
        if let Ok(v) = built {
            crate::globals::global_set(0, "$?", v);
        }
        Ok(())
    }

    fn rb_f_exit(argc: c_int, argv: *const Value) -> Value {
        kernel_call("exit", argc, argv)
    }

    fn rb_f_abort(argc: c_int, argv: *const Value) -> Value {
        kernel_call("abort", argc, argv)
    }

    fn rb_f_exec(argc: c_int, argv: *const Value) -> Value {
        kernel_call("exec", argc, argv)
    }

    fn rb_f_kill(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&send(&class_named("Process")?, "kill", &args)?)
    }

    fn rb_exit(status: c_int) -> () {
        let main = crate::dispatch::main_object();
        crate::dispatch::send_value(
            &main,
            Symbol::intern("exit"),
            &[RubyValue::Int(i64::from(status))],
            None,
        )?;
        Ok(())
    }

    fn rb_get_argv() -> Value {
        to_value(&crate::globals::global_get(0, "$*"))
    }

    fn rb_env_clear() -> Value {
        let env = crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, "ENV")
            .ok_or_else(|| crate::builtins::name_error!("uninitialized constant ENV"))?;
        to_value(&send(&env, "clear", &[])?)
    }

    // ---- the cloexec family ----------------------------------------------

    /// `rb_cloexec_open(path, flags, mode)`: open with `FD_CLOEXEC` set
    /// ATOMICALLY. A separate `fcntl` would leave a window in which a
    /// concurrent `fork` inherits the descriptor, which is the whole reason
    /// MRI has this entry.
    fn rb_cloexec_open(path: *const c_char, flags: c_int, mode: u16) -> c_int {
        let p = unsafe { cstr(path) };
        let Ok(c) = std::ffi::CString::new(p) else {
            return Err(crate::builtins::arg_error!("path contains a null byte"));
        };
        // SAFETY: a NUL-terminated path and the caller's own flags.
        let fd = unsafe { libc::open(c.as_ptr(), flags | libc::O_CLOEXEC, c_int::from(mode)) };
        if fd < 0 {
            return Err(last_errno());
        }
        Ok(fd)
    }

    fn rb_cloexec_dup(fd: c_int) -> c_int {
        // `F_DUPFD_CLOEXEC` is the atomic form, and every platform zeo
        // builds extensions for has it.
        // SAFETY: a plain fcntl on the caller's own descriptor.
        let out = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if out < 0 {
            return Err(last_errno());
        }
        Ok(out)
    }

    /// `rb_cloexec_dup2(old, new)`: `dup2` cannot set the flag atomically on
    /// macOS, so the flag is set right after -- MRI does the same there, and
    /// records the same window.
    fn rb_cloexec_dup2(old: c_int, new: c_int) -> c_int {
        // SAFETY: the caller's own descriptors.
        let out = unsafe { libc::dup2(old, new) };
        if out < 0 {
            return Err(last_errno());
        }
        if old != new {
            // SAFETY: as above.
            unsafe { libc::fcntl(out, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        Ok(out)
    }

    fn rb_cloexec_fcntl_dupfd(fd: c_int, minfd: c_int) -> c_int {
        // SAFETY: the caller's own descriptor.
        let out = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, minfd) };
        if out < 0 {
            return Err(last_errno());
        }
        Ok(out)
    }

    fn rb_cloexec_pipe(fds: *mut c_int) -> c_int {
        if fds.is_null() {
            return Ok(-1);
        }
        let mut pair = [0 as c_int; 2];
        // SAFETY: a two-element array this frame owns.
        if unsafe { libc::pipe(pair.as_mut_ptr()) } < 0 {
            return Err(last_errno());
        }
        for fd in pair {
            // SAFETY: a descriptor `pipe` just handed back.
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        // SAFETY: the caller promised two writable `int`s.
        unsafe {
            fds.write(pair[0]);
            fds.add(1).write(pair[1]);
        }
        Ok(0)
    }

    fn rb_pipe(fds: *mut c_int) -> c_int {
        unsafe { Ok(rb_cloexec_pipe(fds)) }
    }

    /// `rb_fd_fix_cloexec(fd)`: set the flag on a descriptor that came from
    /// somewhere else.
    fn rb_fd_fix_cloexec(fd: c_int) -> () {
        // SAFETY: the caller's own descriptor.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        Ok(())
    }

    /// `rb_reserved_fd_p(fd)`: is this one of the runtime's own descriptors?
    /// MRI reserves the timer thread's pipe; zeo's timer uses a condvar and
    /// reserves nothing, so no descriptor is off limits.
    fn rb_reserved_fd_p(_fd: c_int) -> c_int {
        Ok(0)
    }

    /// `rb_update_max_fd(fd)`: MRI tracks the highest descriptor so it can
    /// close them all at `exec`. zeo sets `FD_CLOEXEC` instead, which is
    /// what makes the bookkeeping unnecessary.
    fn rb_update_max_fd(_fd: c_int) -> () {
        Ok(())
    }

    // ---- the IO an extension was handed ----------------------------------

    /// `rb_io_check_io(v)`: the IO `v` names, through `to_io`, or nil when it
    /// names none.
    ///
    /// MRI writes this as `rb_check_convert_type(v, T_FILE, "IO", "to_io")`
    /// and zeo cannot: an IO reaches C tagged `T_OBJECT`, because a `T_FILE`
    /// tag is a promise that `RFILE(v)->fptr` reads an `rb_io_t`, and zeo has
    /// no `rb_io_t`. So the check is made against zeo's own handle instead,
    /// which answers the same question without the promise.
    fn rb_io_check_io(v: Value) -> Value {
        let val = unsafe { value_of(v) };
        Ok(match io_behind(&val)? {
            Some(io) => to_value(&io)?,
            None => super::value::Q_NIL,
        })
    }

    /// `rb_io_get_io(v)`: the same, raising rather than answering nil.
    fn rb_io_get_io(v: Value) -> Value {
        let val = unsafe { value_of(v) };
        match io_behind(&val)? {
            Some(io) => to_value(&io),
            None => Err(crate::builtins::type_error!(
                "no implicit conversion of {} into IO",
                crate::dispatch::class_name(val.class_id()).unwrap_or("Object".into())
            )),
        }
    }

    /// `rb_io_get_write_io(io)`: the half of a duplex IO that writes.
    ///
    /// MRI answers the io itself unless something tied a separate write half
    /// to it, and zeo has no tied half to find -- see `rb_io_set_write_io`.
    /// The conversion still runs, so a non-IO raises here as it does there.
    fn rb_io_get_write_io(io: Value) -> Value {
        unsafe { Ok(rb_io_get_io(io)) }
    }

    /// `rb_io_set_write_io(io, w)`: tie a separate IO for `io` to write to.
    ///
    /// zeo's duplex handles use ONE descriptor for both directions, so there
    /// is no second object to record and nothing that would consult it: a
    /// write through `io` would keep going to `io`. Recording `w` and then
    /// ignoring it is the silent kind of wrong, so only the calls that ask
    /// for nothing are answered -- untying, or tying an IO to itself -- and
    /// anything else says what it cannot do.
    ///
    /// The answer is the PREVIOUS write half, which for zeo is always nil.
    fn rb_io_set_write_io(io: Value, w: Value) -> Value {
        let target = unsafe { value_of(io) };
        let write = unsafe { value_of(w) };
        if io_behind(&target)?.is_none() {
            return Err(wrong_arg_type(&target, "IO"));
        }
        if matches!(write, RubyValue::Nil | RubyValue::Bool(false)) || io == w {
            return Ok(super::value::Q_NIL);
        }
        Err(crate::builtins::not_impl_error!(
            "zeo cannot tie a separate write IO: its duplex handles read and \
             write one descriptor, so a write through the first IO would not \
             reach the second"
        ))
    }

    /// `rb_io_descriptor(io)`: `IO#fileno`, the entry that replaced reaching
    /// into `fptr->fd`.
    fn rb_io_descriptor(io: Value) -> c_int {
        let target = unsafe { value_of(io) };
        match send(&target, "fileno", &[])? {
            RubyValue::Int(fd) => Ok(fd as c_int),
            other => Err(wrong_arg_type(&other, "Integer")),
        }
    }

    /// `rb_io_mode(io)`: the `FMODE_*` word, the entry that replaced reaching
    /// into `fptr->mode`. See `builtins::io::fmode_bits` for which bits zeo
    /// can answer and which it cannot.
    fn rb_io_mode(io: Value) -> c_int {
        let target = unsafe { value_of(io) };
        if io_behind(&target)?.is_none() {
            return Err(wrong_arg_type(&target, "IO"));
        }
        Ok(crate::builtins::io::fmode_bits(&target) as c_int)
    }

    /// `rb_io_set_timeout(io, timeout)`: `IO#timeout=`, which zeo records and
    /// reads back without enforcing -- its reads block. Stated in
    /// COMPATIBILITY.md, and true whether the setter is Ruby or C.
    fn rb_io_set_timeout(io: Value, timeout: Value) -> Value {
        let target = unsafe { value_of(io) };
        let seconds = unsafe { value_of(timeout) };
        send(&target, "timeout=", &[seconds])?;
        Ok(timeout)
    }

    /// `rb_io_taint_check(io)`: taint is gone from ruby, and what survives of
    /// this entry is the frozen check MRI still makes -- so a `RB_IO_POINTER`
    /// on a frozen IO raises rather than handing out a writable pointer.
    fn rb_io_taint_check(io: Value) -> Value {
        let target = unsafe { value_of(io) };
        let frozen = crate::dispatch::send_value(&target, Symbol::intern("frozen?"), &[], None)?;
        if matches!(frozen, RubyValue::Bool(true)) {
            return Err(crate::builtins::frozen_error!(
                "can't modify frozen {}",
                crate::dispatch::class_name(target.class_id()).unwrap_or("IO".into())
            ));
        }
        Ok(io)
    }

    /// `rb_stat_new(&st)`: a `File::Stat` over a snapshot the extension took
    /// itself, so it reads the fields back through ruby's own accessors
    /// rather than through a struct layout it would have to know.
    fn rb_stat_new(st: *const libc::stat) -> Value {
        if st.is_null() {
            return Err(crate::builtins::arg_error!("rb_stat_new was given no stat"));
        }
        // SAFETY: the caller promised a readable `struct stat`. It is plain
        // old data, so the copy is a memcpy and outlives the caller's buffer.
        to_value(&crate::builtins::stat::stat_from_raw(unsafe { st.read() }))
    }

    // ---- waiting on a descriptor -----------------------------------------

    /// `rb_wait_for_single_fd(fd, events, tv)`: block until one of `events`
    /// is ready, and answer the events that are. `tv` null means no timeout.
    ///
    /// `poll` rather than `select`, which is what MRI uses where it has it:
    /// `select` cannot see a descriptor above `FD_SETSIZE` and writes past
    /// the end of an `fd_set` if handed one.
    fn rb_wait_for_single_fd(fd: c_int, events: c_int, tv: *mut libc::timeval) -> c_int {
        let timeout = if tv.is_null() {
            -1
        } else {
            // SAFETY: the caller promised a readable `struct timeval`.
            let t = unsafe { tv.read() };
            let ms = t.tv_sec * 1000 + crate::usec_i64(t.tv_usec) / 1000;
            c_int::try_from(ms.max(0)).unwrap_or(c_int::MAX)
        };
        let mut pfd = libc::pollfd { fd, events: events as i16, revents: 0 };
        loop {
            // SAFETY: one `pollfd` this frame owns.
            let n = unsafe { libc::poll(&raw mut pfd, 1, timeout) };
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                // The safepoint: an interrupted wait is where `Thread#kill`
                // and `Thread#raise` land, and re-polling without asking
                // would swallow them.
                crate::check_ints()?;
                continue;
            }
            return Ok(if n <= 0 { n } else { c_int::from(pfd.revents) });
        }
    }

    /// `rb_io_wait_readable(fd)`: after a read that failed, should it be
    /// retried?
    ///
    /// The question is asked of `errno`, which is why this takes a bare
    /// descriptor and no IO. `EINTR` answers yes at once, after the
    /// safepoint; `EAGAIN` waits for the descriptor first; anything else is
    /// a real error and answers no, leaving the caller to raise it.
    fn rb_io_wait_readable(fd: c_int) -> c_int {
        wait_retry(fd, libc::POLLIN)
    }

    fn rb_io_wait_writable(fd: c_int) -> c_int {
        wait_retry(fd, libc::POLLOUT)
    }

    /// `rb_fdopen(fd, mode)`: a C stdio stream over an existing descriptor.
    ///
    /// MRI collects and retries once when it runs out of descriptors, because
    /// its own IOs close only when they are collected. zeo's close when they
    /// drop, so there is no reserve for a collection to release and nothing
    /// for a retry to find -- the first failure is the answer.
    fn rb_fdopen(fd: c_int, mode: *const c_char) -> *mut libc::FILE {
        let text = unsafe { cstr(mode) };
        let Ok(c) = std::ffi::CString::new(text) else {
            return Err(crate::builtins::arg_error!("mode contains a null byte"));
        };
        // SAFETY: the caller's own descriptor and a NUL-terminated mode.
        let file = unsafe { libc::fdopen(fd, c.as_ptr()) };
        if file.is_null() {
            return Err(last_errno());
        }
        Ok(file)
    }
}

/// The IO `v` names, through `to_io` when it is not one itself.
///
/// `None` means it names none, which is the difference between
/// `rb_io_check_io` and `rb_io_get_io`. A `to_io` that answers a non-IO is
/// the caller's bug and raises here, exactly as MRI's type check does.
fn io_behind(v: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    if crate::builtins::io::as_rio(v).is_some() {
        return Ok(Some(v.clone()));
    }
    let to_io = Symbol::intern("to_io");
    if !crate::dispatch::responds_to_value(v, to_io, true) {
        return Ok(None);
    }
    let out = crate::dispatch::send_value(v, to_io, &[], None)?;
    if crate::builtins::io::as_rio(&out).is_none() {
        return Err(crate::builtins::type_error!(
            "can't convert {} to IO ({}#to_io gives the wrong type)",
            crate::dispatch::class_name(v.class_id()).unwrap_or("Object".into()),
            crate::dispatch::class_name(v.class_id()).unwrap_or("Object".into())
        ));
    }
    Ok(Some(out))
}

/// The shared half of `rb_io_wait_readable`/`_writable`: read `errno`, and
/// answer whether the caller should try its read or write again.
fn wait_retry(fd: c_int, event: i16) -> Result<c_int, Signal> {
    if fd < 0 {
        return Err(crate::builtins::io_error!("closed stream"));
    }
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    if errno == libc::EINTR {
        crate::check_ints()?;
        return Ok(1);
    }
    if errno != libc::EAGAIN && errno != libc::EWOULDBLOCK {
        return Ok(0);
    }
    let mut pfd = libc::pollfd {
        fd,
        events: event,
        revents: 0,
    };
    loop {
        // SAFETY: one `pollfd` this frame owns. `-1` is "no timeout": the
        // caller asked to wait until the descriptor is ready.
        let n = unsafe { libc::poll(&raw mut pfd, 1, -1) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            crate::check_ints()?;
            continue;
        }
        // Ready, or a poll that failed for a reason the caller's own retry
        // will hit again and report properly.
        return Ok(1);
    }
}

/// The storage behind `GetOpenFile`: one block per IO, zeroed once, refilled
/// on every reach, and kept for the process because the extension keeps the
/// pointer it was handed.
///
/// That last part is why this store outlives a scope where
/// [`super::view`]'s does not. MRI's `rb_io_t` IS the IO's own struct and
/// lives as long as the IO, so an extension holding an `fptr` across calls is
/// doing something MRI supports.
///
/// The key is the `RIo`'s address. An IO that is collected and whose address
/// is later reused hands its block on to the new IO, which is harmless: every
/// field is written before the block is answered.
static IO_SHIMS: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashMap<usize, usize>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// `struct RFile` and the `struct rb_io` its `fptr` names, contiguous so one
/// block serves both. The layout is the vendored header's, generated rather
/// than retyped -- see [`super::layout`].
#[repr(C)]
struct IoShim {
    file: mri::RFile,
    io: mri::rb_io,
}

crate::cext_fn! {
    /// `RFILE(obj)`: the `struct RFile` view over a zeo IO.
    ///
    /// MRI's IO object IS a `struct RFile`, so `RFILE(io)->fptr` is a field
    /// read. A zeo IO is a handle over a payload the runtime owns, so this
    /// mints the two structs beside the object and refills them from the IO
    /// on every reach.
    ///
    /// The view does not write back: `fp->fd = n` changes the view, and the
    /// IO keeps its descriptor. The fields zeo has no answer for -- the read
    /// and write buffers, the converters, the finalizer -- stay zero, so an
    /// extension that reaches into one gets an empty buffer rather than a lie
    /// about its content.
    fn rb_zeo_rfile(obj: Value) -> *mut mri::RFile {
        let target = unsafe { value_of(obj) };
        let Some(rio) = crate::builtins::io::as_rio(&target) else {
            return Err(wrong_arg_type(&target, "IO"));
        };
        let block = shim_for(std::ptr::from_ref(rio) as usize);
        // SAFETY: `shim_for` answers a zeroed block of exactly this layout,
        // kept for the process, and the GVL admits one thread to C at a time.
        unsafe {
            (*block).file.basic = super::view::basic_of(obj);
            (*block).file.fptr = &raw mut (*block).io;
            (*block).io.self_ = obj as mri::VALUE;
            (*block).io.fd = descriptor(&target)?;
            (*block).io.mode = crate::builtins::io::fmode_bits(&target) as mri::rb_io_mode;
            (*block).io.pid = int_reply(&target, "pid")?;
            (*block).io.lineno = int_reply(&target, "lineno")?;
            (*block).io.pathv = to_value(&send(&target, "path", &[])?)? as mri::VALUE;
            (*block).io.tied_io_for_writing = obj as mri::VALUE;
            (*block).io.timeout = to_value(&send(&target, "timeout", &[])?)? as mri::VALUE;
            Ok(&raw mut (*block).file)
        }
    }

    /// The three `rb_io_check_*` entries. Each asks the IO the view was made
    /// from -- never the view's own copy, which an extension may have
    /// overwritten.
    fn rb_io_check_closed(fptr: *mut mri::rb_io) -> () {
        io_check(fptr, Want::Open)
    }

    fn rb_io_check_readable(fptr: *mut mri::rb_io) -> () {
        io_check(fptr, Want::Readable)
    }

    fn rb_io_check_writable(fptr: *mut mri::rb_io) -> () {
        io_check(fptr, Want::Writable)
    }
}

/// The `FMODE_*` bits, as `ruby/io.h` spells them. `fmode_bits` on the
/// builtins side answers the same numbers for an IO's own mode word.
const FMODE_READABLE: c_int = 0x0001;
const FMODE_WRITABLE: c_int = 0x0002;
const FMODE_READWRITE: c_int = FMODE_READABLE | FMODE_WRITABLE;
const FMODE_BINMODE: c_int = 0x0004;
const FMODE_SYNC: c_int = 0x0008;
const FMODE_APPEND: c_int = 0x0040;
const FMODE_CREATE: c_int = 0x0080;
const FMODE_EXCL: c_int = 0x0400;
const FMODE_TRUNC: c_int = 0x0800;
const FMODE_TEXTMODE: c_int = 0x1000;
const FMODE_SETENC_BY_BOM: c_int = 0x0010_0000;

/// `rb_io_modestr_fmode`'s parse, shared with the oflags spelling.
fn modestr_fmode(text: &str) -> Result<c_int, Signal> {
    let bad = || crate::builtins::arg_error!("invalid access mode {text}");
    let mut chars = text.chars();
    let mut fmode = match chars.next() {
        Some('r') => FMODE_READABLE,
        Some('w') => FMODE_WRITABLE | FMODE_TRUNC | FMODE_CREATE,
        Some('a') => FMODE_WRITABLE | FMODE_APPEND | FMODE_CREATE,
        _ => return Err(bad()),
    };
    let rest = chars.as_str();
    for (i, c) in rest.char_indices() {
        match c {
            'b' => fmode |= FMODE_BINMODE,
            't' => fmode |= FMODE_TEXTMODE,
            '+' => fmode |= FMODE_READWRITE,
            'x' => {
                if !text.starts_with('w') {
                    return Err(bad());
                }
                fmode |= FMODE_EXCL;
            }
            ':' => {
                let name = rest[i + 1..].split(':').next().unwrap_or("");
                if name.len() > 4 && name[..4].eq_ignore_ascii_case("bom|") {
                    fmode |= FMODE_SETENC_BY_BOM;
                }
                break;
            }
            _ => return Err(bad()),
        }
    }
    if fmode & FMODE_BINMODE != 0 && fmode & FMODE_TEXTMODE != 0 {
        return Err(bad());
    }
    Ok(fmode)
}

fn fmode_to_oflags(fmode: c_int) -> c_int {
    let mut o = match fmode & FMODE_READWRITE {
        FMODE_READABLE => libc::O_RDONLY,
        FMODE_WRITABLE => libc::O_WRONLY,
        FMODE_READWRITE => libc::O_RDWR,
        _ => 0,
    };
    if fmode & FMODE_APPEND != 0 {
        o |= libc::O_APPEND;
    }
    if fmode & FMODE_TRUNC != 0 {
        o |= libc::O_TRUNC;
    }
    if fmode & FMODE_CREATE != 0 {
        o |= libc::O_CREAT;
    }
    if fmode & FMODE_EXCL != 0 {
        o |= libc::O_EXCL;
    }
    o
}

/// The mode string `fdopen(3)`/`IO.new` want for a set of `FMODE_*` bits --
/// MRI's `rb_io_oflags_modestr`, asked of the fmode side.
fn fmode_modestr(fmode: c_int) -> &'static str {
    let both = fmode & FMODE_READWRITE == FMODE_READWRITE;
    match (fmode & FMODE_APPEND != 0, both, fmode & FMODE_BINMODE != 0) {
        (true, true, true) => "ab+",
        (true, true, false) => "a+",
        (true, false, true) => "ab",
        (true, false, false) => "a",
        (false, true, true) => "rb+",
        (false, true, false) => "r+",
        (false, false, _) if fmode & FMODE_WRITABLE != 0 => {
            if fmode & FMODE_BINMODE != 0 {
                "wb"
            } else {
                "w"
            }
        }
        (false, false, true) => "rb",
        (false, false, false) => "r",
    }
}

/// The IO behind an `fptr`, which the shim stamped with `self_`.
unsafe fn io_of(fptr: *mut mri::rb_io) -> Result<RubyValue, Signal> {
    if fptr.is_null() {
        return Err(crate::builtins::io_error!("uninitialized stream"));
    }
    // SAFETY: a non-null fptr came from `rb_zeo_rfile`, which stamps `self_`.
    Ok(unsafe { value_of((*fptr).self_ as Value) })
}

/// `rb_io_maybe_wait`'s decision, shared by the three spellings. `EINTR`
/// answers 0 after the safepoint, the would-block family forwards to
/// `IO#wait`, and anything else is the caller's error to raise -- MRI's own
/// arms.
fn maybe_wait(error: c_int, io: Value, events: RubyValue, timeout: Value) -> Result<Value, Signal> {
    match error {
        libc::EINTR => {
            crate::check_ints()?;
            to_value(&RubyValue::Int(0))
        }
        libc::EAGAIN | libc::EINPROGRESS => {
            let target = unsafe { value_of(io) };
            let tm = unsafe { value_of(timeout) };
            to_value(&send(&target, "wait", &[events, tm])?)
        }
        _ => Ok(super::value::Q_FALSE),
    }
}

/// The `int` spellings' read of a [`maybe_wait`] answer: the ready events,
/// or 0 for a falsy result.
fn maybe_wait_int(error: c_int, io: Value, events: i64, timeout: Value) -> Result<c_int, Signal> {
    let out = maybe_wait(error, io, RubyValue::Int(events), timeout)?;
    match unsafe { value_of(out) } {
        RubyValue::Int(n) => Ok(n as c_int),
        v if v.truthy() => Err(wrong_arg_type(&v, "Integer")),
        _ => Ok(0),
    }
}

crate::cext_fn! {
    /// `rb_io_modestr_fmode("r+b")` and friends: the mode-string grammar,
    /// including the `x` (`w`-only) and `bom|`-prefix rules.
    fn rb_io_modestr_fmode(modestr: *const c_char) -> c_int {
        modestr_fmode(&unsafe { cstr(modestr) })
    }

    fn rb_io_modestr_oflags(modestr: *const c_char) -> c_int {
        Ok(fmode_to_oflags(modestr_fmode(&unsafe { cstr(modestr) })?))
    }

    fn rb_io_oflags_fmode(oflags: c_int) -> c_int {
        let mut fmode = match oflags & libc::O_ACCMODE {
            libc::O_WRONLY => FMODE_WRITABLE,
            libc::O_RDWR => FMODE_READWRITE,
            _ => FMODE_READABLE,
        };
        if oflags & libc::O_APPEND != 0 {
            fmode |= FMODE_APPEND;
        }
        if oflags & libc::O_TRUNC != 0 {
            fmode |= FMODE_TRUNC;
        }
        if oflags & libc::O_CREAT != 0 {
            fmode |= FMODE_CREATE;
        }
        if oflags & libc::O_EXCL != 0 {
            fmode |= FMODE_EXCL;
        }
        Ok(fmode)
    }

    fn rb_io_check_initialized(fptr: *mut mri::rb_io) -> () {
        if fptr.is_null() {
            return Err(crate::builtins::io_error!("uninitialized stream"));
        }
        Ok(())
    }

    /// zeo reads bytes, never buffered characters, so the byte and char
    /// questions are one question and both are the readable check.
    fn rb_io_check_char_readable(fptr: *mut mri::rb_io) -> () {
        io_check(fptr, Want::Readable)
    }

    fn rb_io_check_byte_readable(fptr: *mut mri::rb_io) -> () {
        io_check(fptr, Want::Readable)
    }

    /// Whether the IO holds read-ahead bytes -- asked of the IO's own
    /// buffer, never the view's empty one, or `gets`-then-`getch` idioms
    /// would wait on a descriptor whose data already arrived.
    fn rb_io_read_pending(fptr: *mut mri::rb_io) -> c_int {
        let target = unsafe { io_of(fptr)? };
        Ok(crate::builtins::io::has_buffered_bytes(&target) as c_int)
    }

    /// `rb_io_read_check`: block until a read would not. Buffered bytes
    /// answer at once; otherwise wait on the descriptor.
    fn rb_io_read_check(fptr: *mut mri::rb_io) -> () {
        let target = unsafe { io_of(fptr)? };
        if crate::builtins::io::has_buffered_bytes(&target) {
            return Ok(());
        }
        // SAFETY: a checked fptr; the fd is the caller's own.
        let fd = unsafe { (*fptr).fd };
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        loop {
            // SAFETY: one `pollfd` this frame owns; -1 waits until ready.
            let n = unsafe { libc::poll(&raw mut pfd, 1, -1) };
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                crate::check_ints()?;
                continue;
            }
            return Ok(());
        }
    }

    fn rb_io_set_nonblock(fptr: *mut mri::rb_io) -> () {
        let target = unsafe { io_of(fptr)? };
        drop(target);
        // SAFETY: fcntl on the caller's descriptor.
        unsafe {
            let fd = (*fptr).fd;
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 {
                return Err(last_errno());
            }
            if flags & libc::O_NONBLOCK == 0
                && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0
            {
                return Err(last_errno());
            }
        }
        Ok(())
    }

    /// `rb_io_synchronized(fptr)`: MRI sets `FMODE_SYNC` in the struct; the
    /// observable is `io.sync`, so that is what is set.
    fn rb_io_synchronized(fptr: *mut mri::rb_io) -> () {
        let target = unsafe { io_of(fptr)? };
        send(&target, "sync=", &[RubyValue::Bool(true)])?;
        Ok(())
    }

    /// `rb_io_fptr_finalize`: close the stream an extension is done with.
    fn rb_io_fptr_finalize(fptr: *mut mri::rb_io) -> c_int {
        let target = unsafe { io_of(fptr)? };
        if !matches!(send(&target, "closed?", &[])?, RubyValue::Bool(true)) {
            send(&target, "close", &[])?;
        }
        Ok(1)
    }

    /// `rb_io_bufwrite(io, buf, size)`: through `IO#write`, so the bytes
    /// land in the same stream order as the Ruby side's own writes.
    fn rb_io_bufwrite(io: Value, buf: *const std::ffi::c_void, size: usize) -> isize {
        let target = unsafe { value_of(io) };
        // SAFETY: the caller's own buffer of `size` bytes.
        let bytes = unsafe { std::slice::from_raw_parts(buf as *const u8, size) }.to_vec();
        let s = RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT));
        match send(&target, "write", &[s])? {
            RubyValue::Int(n) => Ok(n as isize),
            _ => Ok(size as isize),
        }
    }

    fn rb_io_maybe_wait(error: c_int, io: Value, events: Value, timeout: Value) -> Value {
        maybe_wait(error, io, unsafe { value_of(events) }, timeout)
    }

    fn rb_io_maybe_wait_readable(error: c_int, io: Value, timeout: Value) -> c_int {
        maybe_wait_int(error, io, 1, timeout)
    }

    fn rb_io_maybe_wait_writable(error: c_int, io: Value, timeout: Value) -> c_int {
        maybe_wait_int(error, io, 4, timeout)
    }

    /// `rb_io_stdio_file(fptr)`: a C stdio stream over the IO's descriptor,
    /// memoized in the shim block because the caller keeps the pointer.
    fn rb_io_stdio_file(fptr: *mut mri::rb_io) -> *mut libc::FILE {
        // SAFETY: a checked fptr from the shim; the block outlives the IO.
        unsafe {
            if !(*fptr).stdio_file.is_null() {
                return Ok((*fptr).stdio_file.cast());
            }
        }
        let target = unsafe { io_of(fptr)? };
        let fmode = crate::builtins::io::fmode_bits(&target);
        let c = std::ffi::CString::new(fmode_modestr(fmode)).expect("a static mode string");
        // SAFETY: the IO's own descriptor and a NUL-terminated mode.
        let file = unsafe { libc::fdopen((*fptr).fd, c.as_ptr()) };
        if file.is_null() {
            return Err(last_errno());
        }
        // SAFETY: as above; the write memoizes for the next reach.
        unsafe {
            (*fptr).stdio_file = file.cast();
        }
        Ok(file)
    }

    /// `rb_io_open_descriptor(klass, fd, fmode, path, timeout, enc)`: the
    /// modern IO-from-fd constructor (io-console 0.9 uses it for the tty).
    /// Built through `klass.new`, so a subclass's own initialize runs.
    fn rb_io_open_descriptor(
        klass: Value,
        fd: c_int,
        fmode: c_int,
        path: Value,
        timeout: Value,
        _enc: *mut mri::rb_io_encoding,
    ) -> Value {
        let k = unsafe { value_of(klass) };
        let mut args = vec![
            RubyValue::Int(fd as i64),
            a_string(fmode_modestr(fmode)),
        ];
        let path_v = unsafe { value_of(path) };
        if !matches!(path_v, RubyValue::Nil) {
            args.push(RubyValue::Hash(crate::value::collections::hash_new(vec![(
                RubyValue::Symbol(Symbol::intern("path")),
                path_v,
            )])));
        }
        let io = send(&k, "new", &args)?;
        if fmode & FMODE_SYNC != 0 {
            send(&io, "sync=", &[RubyValue::Bool(true)])?;
        }
        let tm = unsafe { value_of(timeout) };
        if !matches!(tm, RubyValue::Nil) {
            send(&io, "timeout=", &[tm])?;
        }
        to_value(&io)
    }
}

/// One zeroed block per IO, at a stable address. See [`IO_SHIMS`].
fn shim_for(key: usize) -> *mut IoShim {
    let mut shims = IO_SHIMS.lock();
    let block = *shims.entry(key).or_insert_with(|| {
        let layout = std::alloc::Layout::new::<IoShim>();
        // SAFETY: `IoShim` is plain data, and all-zero is a valid value of
        // it -- null pointers and zero counts, which is what "zeo has no
        // answer for this field" means.
        unsafe { std::alloc::alloc_zeroed(layout) as usize }
    });
    block as *mut IoShim
}

/// What a `rb_io_check_*` is asking about.
enum Want {
    Open,
    Readable,
    Writable,
}

fn io_check(fptr: *mut mri::rb_io, want: Want) -> Result<(), Signal> {
    const READABLE: i32 = 0x0000_0001;
    const WRITABLE: i32 = 0x0000_0002;
    // SAFETY: `fptr` came from `rb_zeo_rfile`, whose block carries the IO's
    // own `VALUE` in `self`.
    let target = unsafe { value_of((*fptr).self_ as Value) };
    if matches!(send(&target, "closed?", &[])?, RubyValue::Bool(true)) {
        return Err(crate::builtins::io_error!("closed stream"));
    }
    let mode = crate::builtins::io::fmode_bits(&target);
    match want {
        Want::Readable if mode & READABLE == 0 => {
            Err(crate::builtins::io_error!("not opened for reading"))
        }
        Want::Writable if mode & WRITABLE == 0 => {
            Err(crate::builtins::io_error!("not opened for writing"))
        }
        _ => Ok(()),
    }
}

fn descriptor(io: &RubyValue) -> Result<c_int, Signal> {
    match send(io, "fileno", &[])? {
        RubyValue::Int(fd) => Ok(fd as c_int),
        other => Err(wrong_arg_type(&other, "Integer")),
    }
}

/// An `Integer` reply as the C field wants it, and 0 for anything else --
/// which is where CRuby leaves `pid` for a handle that is not a `popen` one.
fn int_reply(io: &RubyValue, meth: &str) -> Result<c_int, Signal> {
    match send(io, meth, &[])? {
        RubyValue::Int(n) => Ok(n as c_int),
        _ => Ok(0),
    }
}

fn io_send(io: Value, meth: &str, argc: c_int, argv: *const Value) -> Result<Value, Signal> {
    let target = unsafe { value_of(io) };
    let args = unsafe { args_of(argc, argv) };
    to_value(&send(&target, meth, &args)?)
}

fn kernel_call(meth: &str, argc: c_int, argv: *const Value) -> Result<Value, Signal> {
    let args = unsafe { args_of(argc, argv) };
    to_value(&crate::dispatch::send_value(
        &crate::dispatch::main_object(),
        Symbol::intern(meth),
        &args,
        None,
    )?)
}

fn file_class_call(meth: &str, args: &[Value]) -> Result<Value, Signal> {
    let vals: Vec<RubyValue> = args.iter().map(|a| unsafe { value_of(*a) }).collect();
    to_value(&send(&class_named("File")?, meth, &vals)?)
}

fn open_file(path: RubyValue, mode: &str) -> Result<Value, Signal> {
    let args = if mode.is_empty() {
        vec![path]
    } else {
        vec![path, a_string(mode)]
    };
    to_value(&send(&class_named("File")?, "open", &args)?)
}

/// Through the Ruby-level `$stderr`, so a redirected one catches it. A
/// raising writer must not turn an error report into an exception, which is
/// why the result is dropped -- the same rule `builtins::warning` follows.
fn write_error(bytes: Vec<u8>) -> Result<(), Signal> {
    let _ = crate::builtins::io::write_bytes(&crate::builtins::io::current_stderr(), &bytes);
    Ok(())
}
