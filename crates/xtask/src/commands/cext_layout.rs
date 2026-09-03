//! `cext layout [--check]`: measure MRI's object layout with a C compiler
//! and record it as `crates/zeo-capi/src/layout_facts.rs`.
//!
//! The C-API crate mirrors the structs an extension reads through a view in
//! hand-written `#[repr(C)]` Rust (`layout.rs`). That Rust cannot be trusted
//! by inspection: a wrong offset reads an unrelated field, silently. So the
//! record here is produced by the only authority there is -- a C compiler
//! reading the pinned headers -- and the crate's own tests hold every field
//! to it. A header bump that moves a field fails `--check` here, by name,
//! before anything else can notice.
//!
//! The same run compiles the extension probe with `-fsyntax-only`: an
//! ordinary extension, written the way real gems write one, still compiles
//! against the tree with every edited macro in play.

use std::path::Path;

use crate::exec::{self, Capture};
use crate::scratch::Scratch;
use crate::{Error, root, root_join, write_if_changed};

const FACTS_RS: &str = "crates/zeo-capi/src/layout_facts.rs";

/// Every struct the views fill, with the fields the Rust mirror names --
/// nested designators for the anonymous unions, as `offsetof` takes them.
const STRUCTS: &[(&str, &[&str])] = &[
    ("struct RBasic", &["flags", "klass"]),
    (
        "struct RString",
        &[
            "basic",
            "len",
            "as.heap.ptr",
            "as.heap.aux.capa",
            "as.heap.aux.shared",
            "as.embed.ary",
        ],
    ),
    (
        "struct RArray",
        &[
            "basic",
            "as.heap.len",
            "as.heap.aux.capa",
            "as.heap.aux.shared_root",
            "as.heap.ptr",
            "as.ary",
        ],
    ),
    ("struct RObject", &["basic", "as.heap.fields", "as.ary"]),
    ("struct RRegexp", &["basic", "ptr", "src", "usecnt"]),
    ("struct RMatch", &["basic", "str", "regexp"]),
    (
        "rb_matchext_t",
        &[
            "regs.allocated",
            "regs.num_regs",
            "regs.beg",
            "regs.end",
            "char_offset",
            "char_offset_num_allocated",
        ],
    ),
    ("struct rmatch_offset", &["beg", "end"]),
    ("struct RFile", &["basic", "fptr"]),
    (
        "struct rb_io_internal_buffer",
        &["ptr", "off", "len", "capa"],
    ),
    (
        "struct rb_io_encoding",
        &["enc", "enc2", "ecflags", "ecopts"],
    ),
    (
        "struct rb_io",
        &[
            "self",
            "stdio_file",
            "fd",
            "mode",
            "pid",
            "lineno",
            "pathv",
            "finalize",
            "wbuf",
            "rbuf",
            "tied_io_for_writing",
            "encs",
            "readconv",
            "cbuf",
            "writeconv",
            "writeconv_asciicompat",
            "writeconv_initialized",
            "writeconv_pre_ecflags",
            "writeconv_pre_ecopts",
            "write_lock",
            "timeout",
        ],
    ),
    ("struct RData", &["basic", "dmark", "dfree", "data"]),
    (
        "struct RTypedData",
        &["basic", "fields_obj", "type", "data"],
    ),
    (
        "rb_data_type_t",
        &[
            "wrap_struct_name",
            "function.dmark",
            "function.dfree",
            "function.dsize",
            "function.dcompact",
            "function.reserved",
            "parent",
            "data",
            "flags",
        ],
    ),
    (
        "struct st_table",
        &[
            "entry_power",
            "bin_power",
            "size_ind",
            "rebuilds_num",
            "type",
            "num_entries",
            "bins",
            "entries_start",
            "entries_bound",
            "entries",
        ],
    ),
];

/// Every constant the C-API crate transcribes by hand.
const CONSTS: &[&str] = &[
    "RUBY_Qfalse",
    "RUBY_Qnil",
    "RUBY_Qtrue",
    "RUBY_Qundef",
    "RUBY_IMMEDIATE_MASK",
    "RUBY_FIXNUM_FLAG",
    "RUBY_FLONUM_MASK",
    "RUBY_FLONUM_FLAG",
    "RUBY_SYMBOL_FLAG",
    "RUBY_T_OBJECT",
    "RUBY_T_CLASS",
    "RUBY_T_MODULE",
    "RUBY_T_FLOAT",
    "RUBY_T_STRING",
    "RUBY_T_REGEXP",
    "RUBY_T_ARRAY",
    "RUBY_T_HASH",
    "RUBY_T_STRUCT",
    "RUBY_T_BIGNUM",
    "RUBY_T_DATA",
    "RUBY_T_MATCH",
    "RUBY_T_COMPLEX",
    "RUBY_T_RATIONAL",
    "RUBY_T_SYMBOL",
    "RUBY_T_MASK",
    "RUBY_FL_FREEZE",
    "RUBY_FL_USER8",
    "RUBY_FL_USER9",
    "RSTRING_NOEMBED",
    "ROBJECT_HEAP",
    "RARRAY_EMBED_FLAG",
    "RUBY_TYPED_FL_IS_TYPED_DATA",
    "RBASIC_SHAPE_ID_FIELD",
];

/// The C that measures. Preprocessor text held as Rust, which is the one
/// accepted residue of C in this repository: it exists so that no `.c` file
/// has to.
fn measuring_probe() -> String {
    let mut c = String::from(
        "#define RUBY_UNTYPED_DATA_WARNING 0\n\
         #include <ruby.h>\n\
         #include <ruby/re.h>\n\
         #include <ruby/io.h>\n\
         #include <ruby/encoding.h>\n\
         #include <ruby/st.h>\n\
         #include <stddef.h>\n\
         #include <stdio.h>\n\
         int main(void)\n{\n",
    );
    for (ty, fields) in STRUCTS {
        c.push_str(&format!(
            "    printf(\"size {ty} %zu\\n\", sizeof({ty}));\n"
        ));
        for f in *fields {
            c.push_str(&format!(
                "    printf(\"field {ty} {f} %zu %zu\\n\", offsetof({ty}, {f}), \
                 sizeof((({ty} *)0)->{f}));\n"
            ));
        }
    }
    for name in CONSTS {
        c.push_str(&format!(
            "    printf(\"const {name} %llu\\n\", (unsigned long long)({name}));\n"
        ));
    }
    c.push_str("    return 0;\n}\n");
    c
}

/// An ordinary extension, compiled for syntax only. Every macro here is one
/// the header hunks edit, both `DATA_PTR(o) = p` lvalue idioms included,
/// and each line of `direct_field_reads` is one a gem in the locked set
/// really writes: a payload struct keeps upstream's layout, so a field read
/// off a view compiles.
const EXTENSION_PROBE: &str = r#"
#define RUBY_UNTYPED_DATA_WARNING 0
#include <ruby.h>
#include <ruby/encoding.h>

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
    if (RB_TYPE_P(s, T_STRING) && !OBJ_FROZEN(s) && RBASIC_CLASS(s) == rb_cString) {
        total += (long)(RBASIC(s)->flags & RUBY_T_MASK);
    }
    total += ENCODING_GET(s) + (long)RB_ENC_CODERANGE(s);
    RB_ENC_CODERANGE_SET(s, RUBY_ENC_CODERANGE_7BIT);
    return LONG2NUM(total);
}

/* date: RTYPEDDATA(self)->data = dat; io-console: RFILE(io)->fptr;
 * strscan: RREGEXP(re)->usecnt++. `RREGEXP(re)->ptr` is absent on purpose:
 * RREGEXP_PTR raises. */
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
    if (0) direct_field_reads(k, k, k, k, k);
}
"#;

/// The compiler an extension would be built with. `CC` wins, as in mkmf.
fn cc() -> String {
    std::env::var("CC").unwrap_or_else(|_| "cc".to_string())
}

pub fn run(check: bool, include: &Path, config: &Path) -> Result<(), Error> {
    let tmp = Scratch::new("cext-layout")?;
    compile_extension_probe(tmp.path(), include, config)?;
    let facts = measure(tmp.path(), include, config)?;
    let rendered = render(&facts);
    if check {
        if std::fs::read(root_join(FACTS_RS)).ok().as_deref() != Some(rendered.as_bytes()) {
            return Err(Error::new(format!(
                "{FACTS_RS} is not what the headers measure -- run `cargo xtask check-c-headers layout`"
            )));
        }
        println!(
            "cext: {} structs, {} fields and {} constants match the headers",
            facts.sizes.len(),
            facts.fields.len(),
            facts.consts.len()
        );
        return Ok(());
    }
    write_if_changed(&root_join(FACTS_RS), rendered.as_bytes())?;
    println!("cext: wrote {FACTS_RS}");
    Ok(())
}

fn compile_extension_probe(tmp: &Path, include: &Path, config: &Path) -> Result<(), Error> {
    let src = tmp.join("extension.c");
    std::fs::write(&src, EXTENSION_PROBE)
        .map_err(|e| Error::new(format!("writing {}: {e}", src.display())))?;
    let out = exec::run(
        &[
            Path::new(&cc()),
            Path::new("-fsyntax-only"),
            Path::new("-Wall"),
            Path::new("-Wextra"),
            Path::new("-I"),
            config,
            Path::new("-I"),
            include,
            &src,
        ],
        root(),
        &[],
        Capture::Both,
    )?;
    let log = out.stderr_text();
    if !out.success() {
        return Err(Error::new(format!(
            "the headers no longer compile a plain C extension:\n{log}"
        )));
    }
    // Upstream warns under -Wextra on its own account; a warning in the
    // probe or in zeo's own header is an edit that changed meaning.
    let ours: Vec<&str> = log
        .lines()
        .filter(|l| l.contains(": warning:"))
        .filter(|l| l.contains("extension.c") || l.contains("internal/zeo.h"))
        .collect();
    if !ours.is_empty() {
        return Err(Error::new(format!(
            "the extension probe warns:\n{}",
            ours.join("\n")
        )));
    }
    Ok(())
}

struct Facts {
    sizes: Vec<(String, usize)>,
    fields: Vec<(String, String, usize, usize)>,
    consts: Vec<(String, u64)>,
}

fn measure(tmp: &Path, include: &Path, config: &Path) -> Result<Facts, Error> {
    let src = tmp.join("measure.c");
    let bin = tmp.join("measure");
    std::fs::write(&src, measuring_probe())
        .map_err(|e| Error::new(format!("writing {}: {e}", src.display())))?;
    let out = exec::run(
        &[
            Path::new(&cc()),
            Path::new("-I"),
            config,
            Path::new("-I"),
            include,
            Path::new("-o"),
            &bin,
            &src,
        ],
        root(),
        &[],
        Capture::Both,
    )?;
    if !out.success() {
        return Err(Error::new(format!(
            "the measuring probe does not compile:\n{}",
            out.stderr_text()
        )));
    }
    let out = exec::run(&[&bin], root(), &[], Capture::Both)?;
    if !out.success() {
        return Err(Error::new("the measuring probe failed to run"));
    }
    let mut facts = Facts {
        sizes: Vec::new(),
        fields: Vec::new(),
        consts: Vec::new(),
    };
    for line in out.stdout_text().lines() {
        let words: Vec<&str> = line.split(' ').collect();
        let bad = || Error::new(format!("the measuring probe printed {line:?}"));
        match words.as_slice() {
            ["size", ty @ .., n] => {
                facts
                    .sizes
                    .push((ty.join(" "), n.parse().map_err(|_| bad())?));
            }
            ["field", rest @ .., offset, size] => {
                let (field, ty) = rest.split_last().ok_or_else(bad)?;
                facts.fields.push((
                    ty.join(" "),
                    field.to_string(),
                    offset.parse().map_err(|_| bad())?,
                    size.parse().map_err(|_| bad())?,
                ));
            }
            ["const", name, v] => {
                facts
                    .consts
                    .push((name.to_string(), v.parse().map_err(|_| bad())?));
            }
            _ => return Err(bad()),
        }
    }
    Ok(facts)
}

fn render(facts: &Facts) -> String {
    let mut s = String::from(
        "//! MRI's object layout as a C compiler measured it from the pinned\n\
         //! headers. GENERATED by `cargo xtask check-c-headers layout`; `layout.rs`'s tests\n\
         //! hold every hand-written struct to these rows.\n\n\
         /// `(type, sizeof)`.\n\
         pub const SIZES: &[(&str, usize)] = &[\n",
    );
    for (ty, n) in &facts.sizes {
        s.push_str(&format!("    ({ty:?}, {n}),\n"));
    }
    s.push_str(
        "];\n\n/// `(type, field, offsetof, sizeof)`.\n\
         pub const FIELDS: &[(&str, &str, usize, usize)] = &[\n",
    );
    for (ty, f, off, size) in &facts.fields {
        s.push_str(&format!("    ({ty:?}, {f:?}, {off}, {size}),\n"));
    }
    s.push_str(
        "];\n\n/// `(name, value)`.\n\
         pub const CONSTS: &[(&str, u64)] = &[\n",
    );
    for (name, v) in &facts.consts {
        s.push_str(&format!("    ({name:?}, {v}),\n"));
    }
    s.push_str(
        "];\n\n/// The value the header gives `name`.\n\
         ///\n\
         /// # Panics\n\
         ///\n\
         /// When `name` was never measured: add it to `cext layout`'s list.\n\
         pub fn measured(name: &str) -> u64 {\n\
         \x20   CONSTS\n\
         \x20       .iter()\n\
         \x20       .find(|(n, _)| *n == name)\n\
         \x20       .map(|(_, v)| *v)\n\
         \x20       .unwrap_or_else(|| panic!(\"{name} was not measured; run `cargo xtask check-c-headers layout`\"))\n\
         }\n",
    );
    s
}
