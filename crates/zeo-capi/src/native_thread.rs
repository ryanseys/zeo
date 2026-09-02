//! `rb_native_mutex_*`, `rb_native_cond_*` and `rb_nativethread_*`: MRI's
//! thin wrappers over pthreads, which an extension uses for its own locks.
//!
//! Each takes a pointer to a real `pthread_mutex_t` or `pthread_cond_t` the
//! extension owns (`rb_nativethread_lock_t` is a typedef of the former), so
//! the libc types are the only honest signatures. A failure is not a Ruby
//! error: it means the extension passed a lock it never initialised or
//! unlocked one it does not hold, and MRI aborts on it too.

use std::ffi::c_int;

use libc::{pthread_cond_t, pthread_mutex_t, pthread_t};

/// MRI's `rb_bug` for these: the message names the call and the errno.
fn fatal(call: &str, err: c_int) -> ! {
    eprintln!(
        "zeo: {call} failed (errno {err}); a C extension passed a lock or condition \
         variable it never initialised, or unlocked one it does not hold"
    );
    std::process::abort()
}

fn checked(call: &str, err: c_int) {
    if err != 0 {
        fatal(call, err);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_mutex_initialize(lock: *mut pthread_mutex_t) {
    checked("pthread_mutex_init", unsafe {
        libc::pthread_mutex_init(lock, std::ptr::null())
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_mutex_destroy(lock: *mut pthread_mutex_t) {
    checked("pthread_mutex_destroy", unsafe {
        libc::pthread_mutex_destroy(lock)
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_mutex_lock(lock: *mut pthread_mutex_t) {
    checked("pthread_mutex_lock", unsafe {
        libc::pthread_mutex_lock(lock)
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_mutex_unlock(lock: *mut pthread_mutex_t) {
    checked("pthread_mutex_unlock", unsafe {
        libc::pthread_mutex_unlock(lock)
    });
}

/// `EBUSY` is the answer "held by someone else", not a failure.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_mutex_trylock(lock: *mut pthread_mutex_t) -> c_int {
    let err = unsafe { libc::pthread_mutex_trylock(lock) };
    if err != 0 && err != libc::EBUSY {
        fatal("pthread_mutex_trylock", err);
    }
    err
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_nativethread_lock_initialize(lock: *mut pthread_mutex_t) {
    unsafe { rb_native_mutex_initialize(lock) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_nativethread_lock_destroy(lock: *mut pthread_mutex_t) {
    unsafe { rb_native_mutex_destroy(lock) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_nativethread_lock_lock(lock: *mut pthread_mutex_t) {
    unsafe { rb_native_mutex_lock(lock) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_nativethread_lock_unlock(lock: *mut pthread_mutex_t) {
    unsafe { rb_native_mutex_unlock(lock) }
}

/// On Linux the condition variable is bound to `CLOCK_MONOTONIC`, so a
/// timed wait is not moved by a wall-clock step; Darwin has a relative-time
/// wait instead ([`rb_native_cond_timedwait`]).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_cond_initialize(cond: *mut pthread_cond_t) {
    #[cfg(target_os = "linux")]
    let err = unsafe {
        let mut attr = std::mem::MaybeUninit::<libc::pthread_condattr_t>::uninit();
        let mut err = libc::pthread_condattr_init(attr.as_mut_ptr());
        if err == 0 {
            err = libc::pthread_condattr_setclock(attr.as_mut_ptr(), libc::CLOCK_MONOTONIC);
        }
        if err == 0 {
            err = libc::pthread_cond_init(cond, attr.as_ptr());
        }
        libc::pthread_condattr_destroy(attr.as_mut_ptr());
        err
    };
    #[cfg(not(target_os = "linux"))]
    let err = unsafe { libc::pthread_cond_init(cond, std::ptr::null()) };
    checked("pthread_cond_init", err);
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_cond_destroy(cond: *mut pthread_cond_t) {
    checked("pthread_cond_destroy", unsafe {
        libc::pthread_cond_destroy(cond)
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_cond_signal(cond: *mut pthread_cond_t) {
    checked("pthread_cond_signal", unsafe {
        libc::pthread_cond_signal(cond)
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_cond_broadcast(cond: *mut pthread_cond_t) {
    checked("pthread_cond_broadcast", unsafe {
        libc::pthread_cond_broadcast(cond)
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_cond_wait(
    cond: *mut pthread_cond_t,
    mutex: *mut pthread_mutex_t,
) {
    let err = unsafe { libc::pthread_cond_wait(cond, mutex) };
    if err != 0 && err != libc::EINTR {
        fatal("pthread_cond_wait", err);
    }
}

/// Wait at most `msec` milliseconds. A timeout is the requested outcome,
/// not an error, and the return type has no room to report it -- MRI's
/// callers re-check their condition.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_native_cond_timedwait(
    cond: *mut pthread_cond_t,
    mutex: *mut pthread_mutex_t,
    msec: libc::c_ulong,
) {
    let err = unsafe { timedwait(cond, mutex, msec) };
    if err != 0 && err != libc::ETIMEDOUT && err != libc::EINTR {
        fatal("pthread_cond_timedwait", err);
    }
}

#[cfg(target_vendor = "apple")]
unsafe fn timedwait(
    cond: *mut pthread_cond_t,
    mutex: *mut pthread_mutex_t,
    msec: libc::c_ulong,
) -> c_int {
    let rel = libc::timespec {
        tv_sec: (msec / 1000) as libc::time_t,
        tv_nsec: ((msec % 1000) * 1_000_000) as libc::c_long,
    };
    unsafe { libc::pthread_cond_timedwait_relative_np(cond, mutex, &rel) }
}

#[cfg(not(target_vendor = "apple"))]
unsafe fn timedwait(
    cond: *mut pthread_cond_t,
    mutex: *mut pthread_mutex_t,
    msec: libc::c_ulong,
) -> c_int {
    // The clock the condition variable was bound to in
    // `rb_native_cond_initialize`.
    #[cfg(target_os = "linux")]
    let clock = libc::CLOCK_MONOTONIC;
    #[cfg(not(target_os = "linux"))]
    let clock = libc::CLOCK_REALTIME;
    let mut deadline = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(clock, &mut deadline) };
    deadline.tv_sec += (msec / 1000) as libc::time_t;
    deadline.tv_nsec += ((msec % 1000) * 1_000_000) as libc::c_long;
    if deadline.tv_nsec >= 1_000_000_000 {
        deadline.tv_nsec -= 1_000_000_000;
        deadline.tv_sec += 1;
    }
    unsafe { libc::pthread_cond_timedwait(cond, mutex, &deadline) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn rb_nativethread_self() -> pthread_t {
    unsafe { libc::pthread_self() }
}
