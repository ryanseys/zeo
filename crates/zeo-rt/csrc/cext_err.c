/*
 * Raising, warning, and `errno`.
 *
 * Two things here cannot be written in Rust.
 *
 * The variadic raises take a format string, so they need `<stdarg.h>` for
 * the same reason `cext_va.c` does. Each formats its message with
 * `zeo_cext_vsnprintf` -- MRI's format, `PRIsVALUE` included -- and hands the
 * result to the Rust entry that builds and throws the exception.
 *
 * `errno` is a macro over a per-thread location, and `rb_errno_ptr` hands out
 * that location's ADDRESS. There is no way to name it from Rust, and reading
 * a copy would give an extension a pointer whose writes go nowhere.
 *
 * Every function that MRI marks `noreturn` is `noreturn` here: the Rust side
 * longjmps, and a compiler that believes the call returns emits dead code
 * after it -- including, in a few gems, a `return` that would answer garbage.
 */

#include <errno.h>
#include <stdarg.h>
#include <stddef.h>

typedef unsigned long VALUE;
typedef unsigned long ID;

/* Implemented in Rust (crates/zeo-rt/src/cext/). */
extern int zeo_cext_vsnprintf(char *buf, size_t cap, const char *fmt, va_list ap);
extern void zeo_cext_warn(const char *msg, int verbose_only, int category);
extern void zeo_cext_sys_warning(const char *msg, int code);
extern void zeo_cext_name_error(VALUE name, const char *msg);
extern void zeo_cext_loaderror(const char *msg, VALUE path);
extern void zeo_cext_frozen_error(VALUE obj, const char *msg);
extern void rb_syserr_fail(int code, const char *msg);
extern void rb_syserr_fail_str(int code, VALUE msg);
extern void rb_mod_syserr_fail(VALUE mod, int code, const char *msg);
extern void rb_mod_syserr_fail_str(VALUE mod, int code, VALUE msg);
extern void rb_readwrite_syserr_fail(int waiting, int code, const char *msg);
extern VALUE rb_id2sym(ID id);
extern void rb_bug(const char *fmt);
extern int ruby_snprintf(char *buf, size_t cap, const char *fmt, ...);

#define ZEO_MSG_MAX 1024

#define ZEO_FORMAT(buf, fmt)                            \
    do {                                                \
        va_list ap;                                     \
        va_start(ap, fmt);                              \
        zeo_cext_vsnprintf((buf), sizeof(buf), (fmt), ap); \
        va_end(ap);                                     \
    } while (0)

/* ---- errno ---------------------------------------------------------- */

int rb_errno(void)
{
    return errno;
}

int *rb_errno_ptr(void)
{
    return &errno;
}

void rb_errno_set(int e)
{
    errno = e;
}

/* ---- the `errno`-reading raises ------------------------------------- */

void rb_sys_fail(const char *msg)
{
    rb_syserr_fail(errno, msg);
}

void rb_sys_fail_str(VALUE msg)
{
    rb_syserr_fail_str(errno, msg);
}

void rb_mod_sys_fail(VALUE mod, const char *msg)
{
    rb_mod_syserr_fail(mod, errno, msg);
}

void rb_mod_sys_fail_str(VALUE mod, VALUE msg)
{
    rb_mod_syserr_fail_str(mod, errno, msg);
}

void rb_readwrite_sys_fail(int waiting, const char *msg)
{
    rb_readwrite_syserr_fail(waiting, errno, msg);
}

/* ---- warnings ------------------------------------------------------- */

void rb_warn(const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_warn(msg, 0, 0);
}

void rb_warning(const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_warn(msg, 1, 0);
}

void rb_category_warn(int cat, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_warn(msg, 0, cat);
}

void rb_category_warning(int cat, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_warn(msg, 1, cat);
}

void rb_sys_warning(const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_sys_warning(msg, errno);
}

/*
 * The compile-time warnings. MRI reports the file and line the PARSER was at;
 * an extension calling one is not parsing anything, so the pair it passes is
 * the only honest location and is written into the message.
 */
void rb_compile_warn(const char *file, int line, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];
    char out[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    ruby_snprintf(out, sizeof(out), "%s:%d: %s", file ? file : "-", line, msg);
    zeo_cext_warn(out, 0, 0);
}

void rb_compile_warning(const char *file, int line, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];
    char out[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    ruby_snprintf(out, sizeof(out), "%s:%d: %s", file ? file : "-", line, msg);
    zeo_cext_warn(out, 1, 0);
}

void rb_category_compile_warn(int cat, const char *file, int line, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];
    char out[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    ruby_snprintf(out, sizeof(out), "%s:%d: %s", file ? file : "-", line, msg);
    zeo_cext_warn(out, 0, cat);
}

/* ---- the named raises ----------------------------------------------- */

void rb_name_error(ID id, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_name_error(rb_id2sym(id), msg);
}

void rb_name_error_str(VALUE name, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_name_error(name, msg);
}

void rb_loaderror(const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_loaderror(msg, (VALUE)0x04 /* Qnil */);
}

void rb_loaderror_with_path(VALUE path, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_loaderror(msg, path);
}

void rb_frozen_error_raise(VALUE obj, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    zeo_cext_frozen_error(obj, msg);
}

/*
 * `rb_assert_failure_detail` is what a failed `RUBY_ASSERT` calls. It is a
 * bug in the extension by construction, so it aborts rather than raising --
 * the same answer `rb_bug` gives.
 */
void rb_assert_failure_detail(const char *file, int line, const char *name,
                              const char *expr, const char *fmt, ...)
{
    char msg[ZEO_MSG_MAX];
    char out[ZEO_MSG_MAX];

    ZEO_FORMAT(msg, fmt);
    ruby_snprintf(out, sizeof(out), "%s:%d:%s: assertion failed: %s: %s",
                  file ? file : "-", line, name ? name : "-",
                  expr ? expr : "-", msg);
    rb_bug(out);
}
