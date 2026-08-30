#include <ruby.h>

/* A module with a nested class whose only class method comes from C -- the
   shape bcrypt has (`BCrypt::Engine.__bc_salt`), and the one that used to
   mint a second `Engine` because zeo registers a compiled nested class by
   qualified NAME rather than through the constant table. */
static VALUE np_salt(int argc, VALUE *argv, VALUE self) {
  (void)argc; (void)argv; (void)self;
  return rb_utf8_str_new("SALT", 4);
}

void Init_nested_probe(void) {
  VALUE mod = rb_define_module("NestedProbe");
  VALUE engine = rb_define_class_under(mod, "Engine", rb_cObject);
  rb_define_singleton_method(engine, "__np_salt", np_salt, -1);
}
