/*
 * The fixture store's buildable extension.
 *
 * One module, one method, one String. Anything more would test the C API
 * rather than the build-and-load path this fixture exists for.
 */

#include <ruby.h>

static VALUE
nativelib_greet(VALUE self)
{
    (void)self;
    return rb_utf8_str_new_cstr("hello from C");
}

void
Init_nativelib(void)
{
    VALUE mod = rb_define_module("Nativelib");

    rb_define_singleton_method(mod, "greet", nativelib_greet, 0);
    rb_define_const(mod, "BUILT", Qtrue);
}
