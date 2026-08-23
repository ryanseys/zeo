//! IO, files, paths and processes.
//!
//! Every entry here is a Ruby method call. zeo's `IO`, `File`, `Dir` and
//! `Process` are real classes over real syscalls, so `rb_io_puts` is
//! `IO#puts` and cannot drift from it -- and the `$stdout` a program
//! reassigned is the one a C extension writes to, which a direct `write(2)`
//! would not be.
//!
//! # The `cloexec` family is the exception
//!
//! `rb_cloexec_open` and its four neighbours are raw file descriptors, not
//! Ruby objects: MRI has them because it must set `FD_CLOEXEC` atomically so
//! a concurrent `fork` cannot leak the descriptor. That reasoning holds here
//! too, so they are real syscalls rather than method calls -- and each uses
//! the atomic form (`O_CLOEXEC`, `dup3`, `F_DUPFD_CLOEXEC`) where the
//! platform has one.

use super::convert::{to_value, value_of};
use super::object::{args_of, cstr, send, wrong_type};
use super::value::Value;
use crate::{RubyValue, Signal, Symbol};
use std::ffi::{c_char, c_int, c_long};

fn class_named(name: &str) -> Result<RubyValue, Signal> {
    crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, name).ok_or_else(|| {
        crate::dispatch::raise_error("NameError", format!("uninitialized constant {name}"))
    })
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
            other => Err(wrong_type(&other, "Integer")),
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
            .ok_or_else(|| crate::dispatch::raise_error("NameError", "uninitialized constant ENV".into()))?;
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
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "path contains a null byte".into(),
            ));
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
