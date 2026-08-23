/*
 * The variadic half of the C API.
 *
 * `rb_funcall(recv, mid, 3, a, b, c)`, `rb_raise(exc, "%s: %d", s, n)`,
 * `rb_scan_args(argc, argv, "11", &a, &b)` and `rb_rescue2(..., cls, 0)` all
 * take arguments whose count and types are described only at run time. Rust
 * cannot read a `va_list`: there is no stable way to, and guessing is how a
 * pointer gets read out of the wrong register.
 *
 * C can. So each of these is written here, where `<stdarg.h>` means what it
 * says, and each does exactly one thing: pack the varargs into an array (or,
 * for `rb_raise`, into a formatted string) and hand that to the Rust entry
 * that does the real work.
 *
 * Nothing here allocates from the Ruby heap, calls back into Ruby, or holds a
 * `VALUE` past its own frame. That keeps the file small enough to read in one
 * sitting, which is the only review a piece of C in this position gets.
 */

#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>

typedef unsigned long VALUE;
typedef unsigned long ID;

/* Implemented in Rust (crates/zeo-rt/src/cext/). */
extern VALUE rb_funcallv(VALUE recv, ID mid, int argc, const VALUE *argv);
extern VALUE zeo_cext_raise_str(VALUE exc, const char *msg);
extern VALUE zeo_cext_rescue2(VALUE (*body)(VALUE), VALUE barg,
                              VALUE (*resc)(VALUE, VALUE), VALUE rarg,
                              const VALUE *classes, int nclasses);
extern int zeo_cext_scan_plan(const char *fmt, int *required, int *optional,
                              int *splat, int *block);
extern VALUE zeo_cext_scan_slice(int argc, const VALUE *argv, int from, int to);
/* `cext_fmt.c`: MRI's format, `PRIsVALUE` included. */
extern int zeo_cext_vsnprintf(char *buf, size_t cap, const char *fmt, va_list ap);

/* MRI's own ceiling: `rb_funcall` past this is a bug in the extension, and
 * MRI's `rb_funcall` has the same fixed buffer for the same reason. */
#define ZEO_MAX_ARGS 64
/* `rb_scan_args` names at most this many slots; MRI's own limit is smaller. */
#define ZEO_MAX_SLOTS 32
/* A `rb_raise` message longer than this is truncated rather than heap-allocated
 * from a function that is about to unwind. MRI truncates at 256. */
#define ZEO_MAX_MSG 1024

VALUE rb_funcall(VALUE recv, ID mid, int n, ...)
{
    VALUE args[ZEO_MAX_ARGS];
    va_list ap;
    int i;

    if (n < 0) {
        n = 0;
    }
    if (n > ZEO_MAX_ARGS) {
        n = ZEO_MAX_ARGS;
    }
    va_start(ap, n);
    for (i = 0; i < n; i++) {
        args[i] = va_arg(ap, VALUE);
    }
    va_end(ap);
    return rb_funcallv(recv, mid, n, args);
}

VALUE rb_funcall2(VALUE recv, ID mid, int argc, const VALUE *argv)
{
    return rb_funcallv(recv, mid, argc, argv);
}

VALUE rb_funcall3(VALUE recv, ID mid, int argc, const VALUE *argv)
{
    return rb_funcallv(recv, mid, argc, argv);
}

/*
 * `rb_raise(exc, fmt, ...)`.
 *
 * The format runs through `zeo_cext_vsnprintf` rather than the system one,
 * because MRI's format is C's plus `PRIsVALUE` -- see `cext_fmt.c`. The
 * `va_list` is read HERE either way: the compiler checked the call site
 * against the prototype, and this is the frame that owns it.
 */
void rb_raise(VALUE exc, const char *fmt, ...)
{
    char msg[ZEO_MAX_MSG];
    va_list ap;

    va_start(ap, fmt);
    zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);
    va_end(ap);
    zeo_cext_raise_str(exc, msg);
    /* Not reached: the Rust side longjmps. */
}

void rb_fatal(const char *fmt, ...)
{
    char msg[ZEO_MAX_MSG];
    va_list ap;

    va_start(ap, fmt);
    zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);
    va_end(ap);
    zeo_cext_raise_str(0, msg);
}

/*
 * `rb_rescue2(body, barg, rescue, rarg, cls1, cls2, ..., 0)`.
 *
 * The class list is NUL-terminated by a literal `0`, so the count is not
 * passed and has to be walked.
 */
VALUE rb_rescue2(VALUE (*body)(VALUE), VALUE barg,
                 VALUE (*resc)(VALUE, VALUE), VALUE rarg, ...)
{
    VALUE classes[ZEO_MAX_ARGS];
    va_list ap;
    int n = 0;
    VALUE c;

    va_start(ap, rarg);
    while (n < ZEO_MAX_ARGS && (c = va_arg(ap, VALUE)) != 0) {
        classes[n++] = c;
    }
    va_end(ap);
    return zeo_cext_rescue2(body, barg, resc, rarg, classes, n);
}

VALUE rb_rescue(VALUE (*body)(VALUE), VALUE barg,
                VALUE (*resc)(VALUE, VALUE), VALUE rarg)
{
    /* No class list means StandardError, which `zeo_cext_rescue2` reads an
     * empty list as. */
    return zeo_cext_rescue2(body, barg, resc, rarg, NULL, 0);
}

/*
 * `rb_scan_args(argc, argv, fmt, ...)`.
 *
 * The format says how many required, optional, splat and block slots follow,
 * and each is a `VALUE *`. Rust parses the format (`zeo_cext_scan_plan`) and
 * slices the arguments (`zeo_cext_scan_slice`); this walks the slots, which
 * is the only part that needs `<stdarg.h>`.
 *
 * Answers how many positional arguments were consumed, as MRI does.
 */
int rb_scan_args(int argc, const VALUE *argv, const char *fmt, ...)
{
    int required = 0, optional = 0, splat = 0, block = 0;
    va_list ap;
    int i, taken, given;
    VALUE *slot;

    if (!zeo_cext_scan_plan(fmt, &required, &optional, &splat, &block)) {
        return 0;
    }
    if (argc < 0) {
        argc = 0;
    }

    va_start(ap, fmt);
    /* Required, then optional: an optional slot with no argument is Qnil. */
    given = argc;
    taken = required + optional;
    if (taken > given) {
        taken = given;
    }
    for (i = 0; i < required + optional; i++) {
        slot = va_arg(ap, VALUE *);
        if (slot != NULL) {
            *slot = (i < taken) ? argv[i] : (VALUE)0x04 /* Qnil */;
        }
    }
    if (splat) {
        slot = va_arg(ap, VALUE *);
        if (slot != NULL) {
            *slot = zeo_cext_scan_slice(argc, argv, taken, argc);
        }
        taken = argc;
    }
    if (block) {
        slot = va_arg(ap, VALUE *);
        if (slot != NULL) {
            *slot = (VALUE)0x04; /* Qnil: the block is not in argv */
        }
    }
    va_end(ap);
    return taken;
}

/* Implemented in Rust: the array-taking workers behind the three variadic
 * Struct entries. */
extern VALUE zeo_cext_struct_define(VALUE outer, const char *name,
                                    const char **members, int n);
extern VALUE zeo_cext_struct_new(VALUE klass, const VALUE *values, int n);
extern long zeo_cext_struct_size(VALUE klass);

/* The member list is a NUL-terminated run of `const char *`. */
static int zeo_collect_names(va_list ap, const char **out)
{
    int n = 0;
    const char *m;

    while (n < ZEO_MAX_SLOTS && (m = va_arg(ap, const char *)) != NULL) {
        out[n++] = m;
    }
    return n;
}

VALUE rb_struct_define(const char *name, ...)
{
    const char *members[ZEO_MAX_SLOTS];
    va_list ap;
    int n;

    va_start(ap, name);
    n = zeo_collect_names(ap, members);
    va_end(ap);
    return zeo_cext_struct_define(0, name, members, n);
}

VALUE rb_struct_define_under(VALUE outer, const char *name, ...)
{
    const char *members[ZEO_MAX_SLOTS];
    va_list ap;
    int n;

    va_start(ap, name);
    n = zeo_collect_names(ap, members);
    va_end(ap);
    return zeo_cext_struct_define(outer, name, members, n);
}

VALUE rb_struct_new(VALUE klass, ...)
{
    VALUE values[ZEO_MAX_ARGS];
    va_list ap;
    int n = 0;
    VALUE v;

    /* Unlike the member list, this one is not terminated: MRI reads exactly
     * as many values as the struct has members. Asking the struct is the only
     * way to know, so the Rust side is handed the count it found. */
    va_start(ap, klass);
    {
        int want = (int)zeo_cext_struct_size(klass);
        while (n < want && n < ZEO_MAX_ARGS) {
            v = va_arg(ap, VALUE);
            values[n++] = v;
        }
    }
    va_end(ap);
    return zeo_cext_struct_new(klass, values, n);
}
