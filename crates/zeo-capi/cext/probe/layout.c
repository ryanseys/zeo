/*
 * Does an ordinary C extension still compile against the patched headers?
 *
 * Every macro here is one the patch series rewrote, so this file is the
 * executable half of `cext/patches/`. It is compiled with `-fsyntax-only`
 * and never linked: what is on trial is the headers, not the runtime.
 *
 * Both lvalue idioms are here on purpose. `DATA_PTR(o) = p` and
 * `RTYPEDDATA_DATA(o) = p` are written that way in real gems, so the
 * accessors hand back a slot rather than a value.
 *
 * `direct_field_reads` is the other half of the rule. A payload struct keeps
 * upstream's layout, so an extension may read one of its fields -- and three
 * gems in the vendored set do. Each line there is one of those gems'.
 */
/* The untyped Data API is deprecated upstream; probing it is the point. */
#define RUBY_UNTYPED_DATA_WARNING 0
#include <ruby.h>

/* zeo's Handle opens with these two words and nothing before them, so a
 * third field or a wider one would silently move `klass` out from under
 * RBASIC_CLASS. `handles.rs` asserts the same two numbers from the Rust
 * side. */
RBIMPL_STATIC_ASSERT(zeo_rbasic_is_two_words,
                     sizeof(struct RBasic) == 2 * sizeof(VALUE));
RBIMPL_STATIC_ASSERT(zeo_rbasic_has_no_shape_id, RBASIC_SHAPE_ID_FIELD == 0);

struct box { VALUE held; int n; };

static void box_mark(void *p) { rb_gc_mark(((struct box *)p)->held); }
static void box_free(void *p) { ruby_xfree(p); }
static size_t box_size(const void *p) { (void)p; return sizeof(struct box); }

static const rb_data_type_t box_type = {
    "zeo/probe/box",
    { box_mark, box_free, box_size, NULL, { NULL } },
    0, 0, RUBY_TYPED_FREE_IMMEDIATELY,
};

static VALUE box_alloc(VALUE klass)
{
    struct box *b;
    return TypedData_Make_Struct(klass, struct box, &box_type, b);
}

static VALUE box_held(VALUE self)
{
    struct box *b;
    TypedData_Get_Struct(self, struct box, &box_type, b);
    return b->held;
}

static VALUE box_slots(VALUE self)
{
    VALUE o = rb_data_object_wrap(rb_cObject, 0, 0, 0);
    DATA_PTR(o) = ruby_xmalloc(8);
    RTYPEDDATA_DATA(self) = DATA_PTR(o);
    (void)RTYPEDDATA_TYPE(self);
    return RTYPEDDATA_P(self) ? Qtrue : Qfalse;
}

static VALUE box_walk(VALUE self, VALUE a, VALUE s, VALUE re, VALUE st)
{
    const VALUE *cp = RARRAY_CONST_PTR(a);
    VALUE *mp = RARRAY_PTR(a);
    long total = RARRAY_LEN(a) + RARRAY_LENINT(a) + RSTRING_LEN(s) + RSTRUCT_LEN(st);

    (void)self;
    (void)cp;
    RARRAY_ASET(a, 0, RARRAY_AREF(a, 0));
    RARRAY_PTR_USE(a, ptr, { total += (long)(ptr - mp); });
    total += RSTRING_END(s) - RSTRING_PTR(s);
    total += RREGEXP_SRC_LEN(re) + RSTRING_LEN(RREGEXP_SRC(re));
    (void)RREGEXP_SRC_PTR(re);
    (void)RREGEXP_SRC_END(re);
    (void)RSTRUCT_GET(st, 0);
    RSTRUCT_SET(st, 0, Qnil);

    /* The RBasic prefix is real, so none of these is patched. */
    if (RB_TYPE_P(s, T_STRING) && !OBJ_FROZEN(s) && RBASIC_CLASS(s) == rb_cString) {
        total += (long)(RBASIC(s)->flags & RUBY_T_MASK);
    }
    return LONG2NUM(total);
}

/*
 * The direct field reads. A payload struct carries upstream's layout, so
 * these compile -- and each one is a line a gem in the vendored set really
 * writes:
 *
 *   date/date_core.c:7615        RTYPEDDATA(self)->data = dat;
 *   io-console/console.c         rb_io_t *fptr = RFILE(io)->fptr;
 *   strscan/strscan.c:629        if (!tmpreg) RREGEXP(re)->usecnt++;
 *
 * `RREGEXP(re)->ptr` is deliberately absent: it is the one field zeo will
 * not hand out, and ::RREGEXP_PTR raises.
 */
static void direct_field_reads(VALUE o, VALUE io, VALUE re, VALUE s, VALUE a)
{
    RTYPEDDATA(o)->data = NULL;
    (void)RDATA(o)->dfree;
    (void)RFILE(io)->fptr;
    RREGEXP(re)->usecnt++;
    (void)RREGEXP(re)->src;
    (void)RSTRING(s)->len;
    (void)RARRAY(a)->basic;
}

void Init_layout(void)
{
    VALUE k = rb_define_class("ZeoCextProbe", rb_cObject);
    rb_define_alloc_func(k, box_alloc);
    rb_define_method(k, "held", box_held, 0);
    rb_define_method(k, "slots", box_slots, 0);
    rb_define_method(k, "walk", box_walk, 4);
    /* Never reached: `k` is a class, not any of the five. The call is here
     * so the compiler counts the function as used. */
    if (0) direct_field_reads(k, k, k, k, k);
}
