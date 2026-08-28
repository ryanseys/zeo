/*
 * `GetOpenFile` -- the `struct RFile` view over a zeo IO.
 *
 * MRI's IO object IS a `struct RFile`, so `RFILE(io)->fptr` is a field read.
 * A zeo IO is a handle over a payload the runtime owns, so there is no such
 * field. This mints the two structs beside the object instead and fills them
 * from the IO each time an extension reaches for them.
 *
 * It is written in C because the layout is the header's, not Rust's: a
 * hand-written `#[repr(C)]` copy of `struct rb_io` would be a second owner of
 * one fact, and a wrong offset here reads or writes an unrelated field. The
 * Rust side owns only the STORAGE (`zeo_cext_io_shim`, one zeroed block per
 * object, kept for the object's life because the extension keeps the
 * pointer) and the questions (`rb_io_descriptor` and friends).
 *
 * The view does not write back. `fp->fd = n` changes the view; the IO keeps
 * its descriptor. The fields zeo has no answer for -- the read and write
 * buffers, the converters, the finalizer -- stay zero, and an extension that
 * reaches into one gets a zeroed buffer rather than a lie about its content.
 */

#include "ruby/ruby.h"
#include "ruby/io.h"

/* The struct's own fields carry MRI's "use the accessor instead" markers.
 * This file IS the accessor's other half, so the markers do not apply. */
#if defined(__GNUC__) || defined(__clang__)
# pragma GCC diagnostic ignored "-Wdeprecated-declarations"
#endif

struct zeo_io_shim {
    struct RFile file;
    struct rb_io io;
};

/* Implemented in Rust (crates/zeo-rt/src/cext/io.rs). */
extern void *zeo_cext_io_shim(VALUE obj, size_t size);
extern int zeo_cext_io_lineno(VALUE obj);
extern int zeo_cext_io_pid(VALUE obj);
extern void zeo_cext_io_check(VALUE obj, int want);

struct RFile *
rb_zeo_rfile(VALUE obj)
{
    struct zeo_io_shim *s = zeo_cext_io_shim(obj, sizeof(struct zeo_io_shim));

    /* `basic` stays zero. A zeo object's flag word is not MRI's, so a copy
     * would read as a T_NONE of class 0 -- which is what it is. */
    s->file.fptr = &s->io;

    s->io.self = obj;
    s->io.fd = rb_io_descriptor(obj);
    s->io.mode = (enum rb_io_mode)rb_io_mode(obj);
    s->io.pid = (rb_pid_t)zeo_cext_io_pid(obj);
    s->io.lineno = zeo_cext_io_lineno(obj);
    s->io.pathv = rb_io_path(obj);
    s->io.tied_io_for_writing = rb_io_get_write_io(obj);
    s->io.timeout = rb_io_timeout(obj);
    return &s->file;
}

/* The three checks `GetOpenFile` and its callers make. Each takes the view,
 * so each asks the IO the view was made from -- never the view's own copy,
 * which an extension may have overwritten. */

void
rb_io_check_closed(rb_io_t *fptr)
{
    zeo_cext_io_check(fptr->self, 0);
}

void
rb_io_check_readable(rb_io_t *fptr)
{
    zeo_cext_io_check(fptr->self, 1);
}

void
rb_io_check_writable(rb_io_t *fptr)
{
    zeo_cext_io_check(fptr->self, 2);
}
