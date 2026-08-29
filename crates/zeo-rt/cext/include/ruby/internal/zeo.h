#ifndef RBIMPL_ZEO_H                                 /*-*-C++-*-vi:se ft=cpp:*/
#define RBIMPL_ZEO_H
/**
 * @file
 * @copyright  Added by zeo. Not part of upstream Ruby.
 * @brief      The view entries every payload-struct cast macro calls.
 *
 * A zeo heap `VALUE` is a handle whose first two words are a real
 * `struct RBasic`. So `RBASIC_CLASS`, `RB_FL_TEST_RAW`, `RB_BUILTIN_TYPE`,
 * `RB_TYPE_P`, `RB_OBJ_FROZEN_RAW` and `RTYPEDDATA_P` read the truth with no
 * patch at all.
 *
 * Behind those two words there is no `struct RString` and no `struct RArray`:
 * a zeo String is an `Arc<Mutex<StrBuf>>` and a zeo Array is a `Vec` the
 * runtime owns. So a cast to a payload struct would read bytes that mean
 * nothing, and every cast macro calls in here instead.
 *
 * One rule governs all of them:
 *
 *   A payload struct carries UPSTREAM'S LAYOUT. `X(obj)` is a call, not a
 *   cast, and answers a view the runtime owns. The view ALIASES real storage
 *   wherever zeo owns that storage as C-shaped memory, and is REFILLED from
 *   the object on every reach wherever it does not. A field zeo has no answer
 *   for is zero, and the accessor named for it raises.
 *
 * `rb_zeo_rdata` and `rb_zeo_rtypeddata` are the aliasing pair: the cell they
 * answer lives inside the object, so `RTYPEDDATA(v)->data = p` writes the
 * object's own slot and cannot go stale. Every other view is a refill, so a
 * pointer held across a call into Ruby reads what the object looked like at
 * the reach -- MRI gives the same warning about its own `RSTRING_PTR`.
 *
 * A refilled view does not write back. `RSTRING(s)->len = 3` changes the
 * view; the String keeps its length. The BYTES `RSTRING_PTR` answers are the
 * String's own, so writing through that pointer does reach it.
 */
#include "ruby/internal/dllexport.h"
#include "ruby/internal/value.h"

struct RString;
struct RArray;
struct RObject;
struct RRegexp;
struct RMatch;
struct RFile;
struct RData;
struct RTypedData;

RBIMPL_SYMBOL_EXPORT_BEGIN()

/* Refilled views. Each raises if the object is not of that type. */
struct RString *rb_zeo_rstring(VALUE obj);
struct RArray *rb_zeo_rarray(VALUE obj);
struct RObject *rb_zeo_robject(VALUE obj);
struct RRegexp *rb_zeo_rregexp(VALUE obj);
struct RMatch *rb_zeo_rmatch(VALUE obj);
struct RFile *rb_zeo_rfile(VALUE obj);

/* Aliasing views over the object's own cell. */
struct RData *rb_zeo_rdata(VALUE obj);
struct RTypedData *rb_zeo_rtypeddata(VALUE obj);

/* `RARRAY_ASET`, which must reach the Array. The pointer `RARRAY_PTR_USE`
 * hands out is a projection of tagged `VALUE`s built beside the object -- a
 * zeo Array element is a Rust enum, not a word -- so a store through it stays
 * in the projection. This one goes to the Array. */
void rb_zeo_ary_aset(VALUE ary, long i, VALUE v);

/* The loud floor: a field zeo will not hand out. Raises
 * `NotImplementedError` naming the macro that reached it. The return type
 * only satisfies the caller. */
void *rb_zeo_no_field(const char *what);

RBIMPL_SYMBOL_EXPORT_END()

#endif /* RBIMPL_ZEO_H */
