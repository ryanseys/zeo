#include <ruby.h>
static VALUE probe_hi(VALUE self) { (void)self; return rb_utf8_str_new("hi", 2); }
void Init_probe(void) { rb_define_global_function("probe_hi", probe_hi, 0); }
