/*
 * The native mutex and condition variable an extension locks by hand.
 *
 * These are NOT Ruby's `Thread::Mutex`. `ruby/thread_native.h` says so in as
 * many words -- "if you want to use Ruby's Mutex and so on to synchronize Ruby
 * Threads, please use Mutex directly" -- and the difference is load bearing: a
 * `Thread::Mutex` parks a Ruby thread and releases the GVL, while these park
 * the OS thread and hold whatever the caller was holding. An extension reaches
 * for them to guard its OWN C state, usually inside a
 * `rb_thread_call_without_gvl` body where no Ruby object is in sight.
 *
 * On every platform zeo builds extensions for, `ruby/thread_native.h` typedefs
 * the three types straight to pthreads:
 *
 *     rb_nativethread_id_t   = pthread_t
 *     rb_nativethread_lock_t = pthread_mutex_t
 *     rb_nativethread_cond_t = pthread_cond_t
 *
 * So each entry is one libc call on the caller's own object, and this file is
 * C rather than Rust for exactly that reason: the pointer an extension hands
 * in points at a real `pthread_mutex_t`, and C is where that type is defined.
 *
 * `rb_nativethread_lock_*` and `rb_native_mutex_*` are the same five functions
 * under two names -- the header spells the second set `@alias{}` of the first
 * -- so each pair shares one body.
 *
 * # What a failure does
 *
 * A `pthread_mutex_lock` that fails means the caller passed something that is
 * not an initialised mutex, or unlocked one it does not own. There is nothing
 * to raise INTO: these run with no Ruby frame and often with the GVL released,
 * and the return type is `void`, so an error cannot be reported to the caller
 * either. Continuing past one means running the critical section unlocked. MRI
 * calls `rb_bug` here; zeo aborts with the same reasoning and names the call.
 */

#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static void fatal(const char *call, int err)
{
    fprintf(stderr, "zeo: %s failed (errno %d); a C extension passed a lock or "
                    "condition variable it never initialised, or unlocked one "
                    "it does not hold\n",
            call, err);
    abort();
}

/* ---- mutexes --------------------------------------------------------- */

void rb_native_mutex_initialize(pthread_mutex_t *lock)
{
    int err = pthread_mutex_init(lock, NULL);
    if (err != 0) {
        fatal("pthread_mutex_init", err);
    }
}

void rb_native_mutex_destroy(pthread_mutex_t *lock)
{
    int err = pthread_mutex_destroy(lock);
    if (err != 0) {
        fatal("pthread_mutex_destroy", err);
    }
}

void rb_native_mutex_lock(pthread_mutex_t *lock)
{
    int err = pthread_mutex_lock(lock);
    if (err != 0) {
        fatal("pthread_mutex_lock", err);
    }
}

void rb_native_mutex_unlock(pthread_mutex_t *lock)
{
    int err = pthread_mutex_unlock(lock);
    if (err != 0) {
        fatal("pthread_mutex_unlock", err);
    }
}

/*
 * The one that answers rather than aborts: `EBUSY` is the point of the call,
 * and the header documents it as a return value.
 */
int rb_native_mutex_trylock(pthread_mutex_t *lock)
{
    int err = pthread_mutex_trylock(lock);
    if (err != 0 && err != EBUSY) {
        fatal("pthread_mutex_trylock", err);
    }
    return err;
}

void rb_nativethread_lock_initialize(pthread_mutex_t *lock)
{
    rb_native_mutex_initialize(lock);
}

void rb_nativethread_lock_destroy(pthread_mutex_t *lock)
{
    rb_native_mutex_destroy(lock);
}

void rb_nativethread_lock_lock(pthread_mutex_t *lock)
{
    rb_native_mutex_lock(lock);
}

void rb_nativethread_lock_unlock(pthread_mutex_t *lock)
{
    rb_native_mutex_unlock(lock);
}

/* ---- condition variables --------------------------------------------- */

/*
 * A relative timeout has to be measured against SOME clock, and the clock a
 * `pthread_cond_timedwait` deadline is read against is fixed when the
 * condition variable is created. Getting the two out of step is not a slow
 * wait -- it is a wait that returns at once or one that never returns.
 *
 * macOS has no `pthread_condattr_setclock`, but it has
 * `pthread_cond_timedwait_relative_np`, which takes the interval directly and
 * so never names a clock at all. Linux has the attribute, so the condition
 * variable is created against `CLOCK_MONOTONIC` and the deadline is read from
 * the same one -- a wall clock the operator moves must not lengthen or
 * shorten a timeout. That is MRI's own arrangement, and it carries MRI's one
 * assumption with it: a condition variable reaching `rb_native_cond_timedwait`
 * came from `rb_native_cond_initialize`. A statically initialised
 * `PTHREAD_COND_INITIALIZER` runs on the real-time clock, and neither MRI nor
 * zeo can tell one from the other through the pointer.
 */
void rb_native_cond_initialize(pthread_cond_t *cond)
{
#if defined(__linux__)
    pthread_condattr_t attr;
    int err = pthread_condattr_init(&attr);
    if (err == 0) {
        err = pthread_condattr_setclock(&attr, CLOCK_MONOTONIC);
    }
    if (err == 0) {
        err = pthread_cond_init(cond, &attr);
    }
    pthread_condattr_destroy(&attr);
#else
    int err = pthread_cond_init(cond, NULL);
#endif
    if (err != 0) {
        fatal("pthread_cond_init", err);
    }
}

void rb_native_cond_destroy(pthread_cond_t *cond)
{
    int err = pthread_cond_destroy(cond);
    if (err != 0) {
        fatal("pthread_cond_destroy", err);
    }
}

void rb_native_cond_signal(pthread_cond_t *cond)
{
    int err = pthread_cond_signal(cond);
    if (err != 0) {
        fatal("pthread_cond_signal", err);
    }
}

void rb_native_cond_broadcast(pthread_cond_t *cond)
{
    int err = pthread_cond_broadcast(cond);
    if (err != 0) {
        fatal("pthread_cond_broadcast", err);
    }
}

/*
 * `EINTR` is not a failure and not a timeout: it is the spurious wakeup the
 * header tells callers to brace for, and the caller's own loop re-checks its
 * predicate. Passing it back as a normal return is what lets that loop work.
 */
void rb_native_cond_wait(pthread_cond_t *cond, pthread_mutex_t *mutex)
{
    int err = pthread_cond_wait(cond, mutex);
    if (err != 0 && err != EINTR) {
        fatal("pthread_cond_wait", err);
    }
}

void rb_native_cond_timedwait(pthread_cond_t *cond, pthread_mutex_t *mutex,
                              unsigned long msec)
{
    int err;
#if defined(__APPLE__)
    struct timespec rel;
    rel.tv_sec = (time_t)(msec / 1000UL);
    rel.tv_nsec = (long)((msec % 1000UL) * 1000000UL);
    err = pthread_cond_timedwait_relative_np(cond, mutex, &rel);
#else
    struct timespec deadline;
#if defined(__linux__)
    clock_gettime(CLOCK_MONOTONIC, &deadline);
#else
    clock_gettime(CLOCK_REALTIME, &deadline);
#endif
    deadline.tv_sec += (time_t)(msec / 1000UL);
    deadline.tv_nsec += (long)((msec % 1000UL) * 1000000UL);
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_nsec -= 1000000000L;
        deadline.tv_sec += 1;
    }
    err = pthread_cond_timedwait(cond, mutex, &deadline);
#endif
    /* A timeout is the requested outcome, not an error, and the return type
     * has nowhere to report it -- the caller re-checks its own predicate. */
    if (err != 0 && err != ETIMEDOUT && err != EINTR) {
        fatal("pthread_cond_timedwait", err);
    }
}

/* ---- thread identity -------------------------------------------------- */

/*
 * An extension calls this to key its own per-thread table, and compares two
 * answers with `pthread_equal`. zeo runs every Ruby thread on its own OS
 * thread, so the identity it hands back stays one-to-one with `Thread.current`
 * for as long as that thread lives.
 */
pthread_t rb_nativethread_self(void)
{
    return pthread_self();
}
