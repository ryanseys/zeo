#ifndef RBIMPL_ZEO_H                                 /*-*-C++-*-vi:se ft=cpp:*/
#define RBIMPL_ZEO_H
/**
 * @file
 * @copyright  Added by zeo. Not part of upstream Ruby.
 * @brief      The accessors zeo answers in place of MRI's object layout.
 *
 * A zeo heap `VALUE` is a handle whose first two words are a real
 * `struct RBasic`. So `RBASIC_CLASS`, `RB_FL_TEST_RAW`, `RB_BUILTIN_TYPE`,
 * `RB_TYPE_P`, `RB_OBJ_FROZEN_RAW` and `RTYPEDDATA_P` read the truth with no
 * patch at all, and an extension that sets `FL_USER3` on its own object
 * writes the word that owns it.
 *
 * Behind those two words there is no `struct RString` and no `struct RArray`.
 * A zeo String is an `Arc<Mutex<StrBuf>>` and a zeo Array is a `Vec` the
 * runtime owns, so every reader of that half is patched to call in here.
 *
 * ::rbimpl_zeo_unsupported_ptr raises `NotImplementedError` naming the macro
 * that reached it. A layout reader is never left reading a struct: an
 * unimplemented one fails loudly at the call and never quietly answers a
 * wrong byte.
 */
#include "ruby/internal/dllexport.h"
#include "ruby/internal/value.h"

struct rb_data_type_struct;
struct re_registers;

RBIMPL_SYMBOL_EXPORT_BEGIN()

/* String. The pointer is writable and stays valid for the enclosing scope. */
char *rbimpl_zeo_str_ptr(VALUE str);
long rbimpl_zeo_str_len(VALUE str);

/* Array. `const_ptr` and `ptr` materialize a contiguous view and pin it for
 * the enclosing scope; `RARRAY_PTR_USE` keeps upstream's
 * `rb_ary_ptr_use_start`/`_end` pair and is not patched. */
long rbimpl_zeo_ary_len(VALUE ary);
VALUE rbimpl_zeo_ary_aref(VALUE ary, long i);
void rbimpl_zeo_ary_aset(VALUE ary, long i, VALUE v);
const VALUE *rbimpl_zeo_ary_const_ptr(VALUE ary);
VALUE *rbimpl_zeo_ary_ptr(VALUE ary);

/* Data and TypedData. `DATA_PTR` and `RTYPEDDATA_DATA` are assigned through
 * in the wild, so the slot is what the accessor hands back. */
void **rbimpl_zeo_data_slot(VALUE obj);
const struct rb_data_type_struct *rbimpl_zeo_typeddata_type(VALUE obj);

/* Regexp and MatchData. zeo's engine is Oniguruma, so the registers and the
 * compiled pattern are real. `RREGEXP_PTR` is assigned through by strscan's
 * pre-3.3 shim, so it too is a slot. */
VALUE rbimpl_zeo_regexp_src(VALUE re);
void **rbimpl_zeo_regexp_ptr_slot(VALUE re);
struct re_registers *rbimpl_zeo_match_regs(VALUE match);

/* The loud floor. Raises; the return type only satisfies the caller. */
void *rbimpl_zeo_unsupported_ptr(const char *what);

RBIMPL_SYMBOL_EXPORT_END()

#endif /* RBIMPL_ZEO_H */
