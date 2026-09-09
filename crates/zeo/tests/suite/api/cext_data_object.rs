//! A `dup` of a C data object, against ruby's recorded answer.
//!
//! This was the one golden with a C extension beside it. A golden with an
//! extension compares two BUILDS -- one against MRI's headers for ruby, one
//! against zeo's -- so its answer could only be recorded by `cargo xtask
//! bless`, never by the live oracle, and the harness carried a whole road
//! for that one case. Ruby's answer is written here instead, once, and the
//! extension is text (`checks/no_c.rs`).

use std::path::Path;

use crate::support::{compile_link_run, extension_dir, have};

/// A minimal TypedData class. `value` reads `DATA_PTR` by hand rather than
/// through `TypedData_Get_Struct`, because the whole question is what that
/// pointer IS after a `dup` -- and the accessor raises on the answer this
/// asks about.
const DUP_DATA_C: &str = r#"#include <ruby.h>

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
"#;

const PROGRAM: &str = r#"require "dup_data"

a = DupData.new(7)
b = a.dup
c = a.clone

p [a.value, a.has_struct?]
p [b.value, b.has_struct?]
p [c.value, c.has_struct?]
p [a.class, b.class, c.class]
"#;

/// ruby 4.0.6's answer: a copy runs the class's alloc
/// function and gets a fresh zeroed struct of its own -- not the original's,
/// and not none.
#[test]
fn a_dup_of_a_c_data_object_holds_a_fresh_struct() {
    if !have("cc") {
        eprintln!("skipping: this machine has no `cc`");
        return;
    }
    let dir = extension_dir("dup-data", "dup_data", DUP_DATA_C);
    let zeo = crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"));
    zeo::cext::configure(&zeo, &dir, Path::new("extconf.rb"), &[])
        .unwrap_or_else(|e| panic!("{e}"));
    zeo::cext::build_extension(&dir, 4).unwrap_or_else(|e| panic!("{e}"));

    let opts = zeo::CompileOptions {
        load_roots: vec![dir],
        ..Default::default()
    };
    let out = compile_link_run(PROGRAM, &opts, &[], &[]);
    assert_eq!(
        out.stdout, "[7, true]\n[0, true]\n[0, true]\n[DupData, DupData, DupData]\n",
        "stderr: {}",
        out.stderr
    );
}
