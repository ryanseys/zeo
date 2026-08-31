/* A minimal TypedData class, for the `dup` divergence.
 *
 * `value` reads DATA_PTR by hand rather than through TypedData_Get_Struct,
 * because the whole question is what that pointer IS after a `dup` -- and the
 * accessor raises on the answer this is asking about. */
#include <ruby.h>

struct dd {
    int n;
};

static void dd_free(void *p) { xfree(p); }

static size_t dd_size(const void *p) { (void)p; return sizeof(struct dd); }

static const rb_data_type_t dd_type = {
    "dup_data/DupData",
    { NULL, dd_free, dd_size },
    NULL, NULL,
    RUBY_TYPED_FREE_IMMEDIATELY,
};

static VALUE dd_alloc(VALUE klass) {
    struct dd *p;
    return TypedData_Make_Struct(klass, struct dd, &dd_type, p);
}

static VALUE dd_initialize(VALUE self, VALUE n) {
    struct dd *p = (struct dd *)DATA_PTR(self);
    p->n = NUM2INT(n);
    return self;
}

static VALUE dd_value(VALUE self) {
    struct dd *p = (struct dd *)DATA_PTR(self);
    if (!p) return Qnil;
    return INT2NUM(p->n);
}

static VALUE dd_has_struct(VALUE self) {
    return DATA_PTR(self) ? Qtrue : Qfalse;
}

void Init_dup_data(void) {
    VALUE cls = rb_define_class("DupData", rb_cObject);
    rb_define_alloc_func(cls, dd_alloc);
    rb_define_method(cls, "initialize", dd_initialize, 1);
    rb_define_method(cls, "value", dd_value, 0);
    rb_define_method(cls, "has_struct?", dd_has_struct, 0);
}
