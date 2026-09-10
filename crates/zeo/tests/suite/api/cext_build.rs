//! A C extension configures and builds under zeo, end to end.
//!
//! This is the only test that exercises the whole build pipeline at once:
//! `extconf.rb` runs under zeo, which means `mkmf` loads and probes; mkmf
//! writes a Makefile; `make` compiles the C against MRI's fetched headers;
//! and the link produces a loadable bundle whose only undefined symbols are
//! the runtime's own.
//!
//! zeo drives the compile and the link itself: `make` appears here only to
//! prove the fallback still works, and never on the ordinary path.
//!
//! Every step failed for its own reason while this was built, and none of
//! them would have been caught by anything else in the suite:
//!
//! * `RbConfig.expand` returned a new String where mkmf needs it to MUTATE in
//!   place, so `srcdir` stayed the literal `$(srcdir)` and every Makefile
//!   came out with an empty `SRCS`.
//! * `Dir["./*.c"]` answered `[]`, because a `.` path segment was matched as
//!   a directory name instead of being carried as display text.
//! * `DLDFLAGS` had no `-undefined dynamic_lookup`, so the link failed with
//!   "Undefined symbols ... _rb_define_method" -- which reads like a missing
//!   implementation and is only a link-line flag.
//!
//! A unit test can see none of those. This can.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::support::{extension_dir, have};

/// The `zeo` binary beside this test binary's profile dir -- unlike the
/// old `target/{debug,release}` guess, this survives CARGO_TARGET_DIR and
/// custom profiles.
fn zeo_bin() -> PathBuf {
    crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"))
}

/// One global function, from C. The smallest extension the build path can
/// prove itself on.
const PROBE_C: &str = r#"#include <ruby.h>
static VALUE probe_hi(VALUE self) { (void)self; return rb_utf8_str_new("hi", 2); }
void Init_probe(void) { rb_define_global_function("probe_hi", probe_hi, 0); }
"#;

/// A module with a nested class whose only class method comes from C -- the
/// shape bcrypt has (`BCrypt::Engine.__bc_salt`), and the one that used to
/// mint a second `Engine` because zeo registered a compiled nested class by
/// qualified NAME rather than through the constant table.
const NESTED_PROBE_C: &str = r#"#include <ruby.h>

static VALUE np_salt(int argc, VALUE *argv, VALUE self) {
  (void)argc; (void)argv; (void)self;
  return rb_utf8_str_new("SALT", 4);
}

void Init_nested_probe(void) {
  VALUE mod = rb_define_module("NestedProbe");
  VALUE engine = rb_define_class_under(mod, "Engine", rb_cObject);
  rb_define_singleton_method(engine, "__np_salt", np_salt, -1);
}
"#;

/// `RbConfig.ruby` names the zeo that is running, and that file exists.
///
/// rubygems spawns exactly this string to run a gem's `extconf.rb`
/// (`Gem.ruby` is `RbConfig.ruby`). A shim that synthesized the path from a
/// hardcoded FHS prefix would make every gem with a C extension die at
/// `extconf failed: No such file or directory` while pure-ruby gems install
/// fine -- a failure that names a path nobody in the repo ever wrote.
///
/// The three keys are asserted separately because `bindir` and
/// `ruby_install_name` are what mkmf and rubygems read directly; a fix that
/// only patched the joined `RbConfig.ruby` would leave both wrong.
#[test]
fn rbconfig_names_the_running_zeo_as_the_interpreter() {
    let zeo = zeo_bin();
    let out = Command::new(&zeo)
        .arg("-e")
        .arg(
            r#"require "rbconfig"
puts RbConfig.ruby
puts RbConfig::CONFIG["bindir"]
puts RbConfig::CONFIG["ruby_install_name"]
puts File.executable?(RbConfig.ruby)
"#,
        )
        // Ambient ruby config must not reach the parse.
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("zeo runs");
    assert!(
        out.status.success(),
        "zeo failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 4, "unexpected output:\n{stdout}");

    // Canonicalized on both sides: the test binary is routinely reached
    // through a symlinked target dir, and the shim reports the real path.
    let want = zeo.canonicalize().expect("the zeo binary is reachable");
    assert_eq!(
        Path::new(lines[0]).canonicalize().ok().as_deref(),
        Some(want.as_path()),
        "RbConfig.ruby is `{}`, not this zeo:\n{stdout}",
        lines[0]
    );
    assert_eq!(Some(Path::new(lines[1])), want.parent(), "bindir");
    assert_eq!(
        want.file_name().and_then(|n| n.to_str()),
        Some(lines[2]),
        "ruby_install_name"
    );
    assert_eq!(lines[3], "true", "RbConfig.ruby names a file that runs");
}

/// FLAKY under a loaded machine: a full `-p zeo` run can fail here with
/// "extconf.rb did not produce a Makefile" after 1.5s, where a passing run
/// takes 8-19s. It passes on its own every time. Cause NOT established --
/// the scratch directory is keyed on the pid alone, which is the shape of a
/// collision, so that is where to look first. Re-run before believing a
/// failure here names a real defect.
#[test]
fn an_extension_configures_compiles_and_links() {
    // `make` and a C compiler are what an extension build IS. A machine
    // without them cannot run this, and saying so beats a confusing failure.
    if !have("make") || !have("cc") {
        eprintln!("skipping: this machine has no `make` or no `cc`");
        return;
    }

    let dir = extension_dir("build", "probe", PROBE_C);

    // `zeo::cext::configure` re-enters zeo as a subprocess and exports the
    // header directories, which is the whole path an installed zeo takes --
    // spawning the binary by hand here would skip the export and pass only
    // because the dev tree's fallback happens to be right.
    zeo::cext::configure(&zeo_bin(), &dir, Path::new("extconf.rb"), &[])
        .unwrap_or_else(|e| panic!("{e}"));

    let makefile = std::fs::read_to_string(dir.join("Makefile")).expect("mkmf wrote a Makefile");
    // The three lines that were empty when `RbConfig.expand` did not mutate.
    // A Makefile with no sources still "builds" -- it just makes nothing.
    for want in ["ORIG_SRCS = probe.c", "OBJS = probe.o", "TARGET = probe"] {
        assert!(
            makefile.lines().any(|l| l.trim_end() == want),
            "the Makefile has no `{want}` line:\n{makefile}"
        );
    }

    // ZEO drives the build. `make` is not on this path at all -- see
    // `crates/zeo/src/cext/build.rs` for the two commands and for when it
    // still hands over.
    let built = zeo::cext::build_extension(&dir, 4).unwrap_or_else(|e| panic!("{e}"));

    // `DLEXT`: `bundle` on macOS, `so` everywhere else -- the same split
    // `crates/zeo/build.rs` renders into the rbconfig shim.
    let dlext = if cfg!(target_vendor = "apple") {
        "bundle"
    } else {
        "so"
    };
    let bundle = dir.join(format!("probe.{dlext}"));
    assert_eq!(built, bundle, "the driver named a different product");
    assert!(bundle.is_file(), "{} was not produced", bundle.display());
    assert!(dir.join("probe.o").is_file(), "the object file is missing");

    // The `make` fallback has to work too, and it is the path a gem with a
    // custom rule takes -- so it is exercised rather than assumed.
    for f in ["probe.o", &format!("probe.{dlext}")] {
        std::fs::remove_file(dir.join(f)).unwrap_or_else(|e| panic!("removing {f}: {e}"));
    }
    let made = Command::new("make")
        .current_dir(&dir)
        .output()
        .expect("make runs");
    assert!(
        bundle.is_file(),
        "make did not produce {}:\n{}\n{}",
        bundle.display(),
        String::from_utf8_lossy(&made.stdout),
        String::from_utf8_lossy(&made.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A COMPUTED `require` reaches a compiled extension on `$LOAD_PATH`.
///
/// The literal spelling is the compile-time loader's: it maps the feature
/// onto a store gem, builds it, and splices a `CExtLoaded` node. A computed
/// one names a feature no compile can see, so it has to resolve and dlopen
/// at RUN time, and the run-time resolver must try the native suffix, not
/// only `.rb` and the verbatim name read as text. A resolver that reads a
/// bundle as text fails with "stream did not contain valid UTF-8", which
/// reads like a corrupt file rather than a loader that took the wrong
/// branch.
///
/// Four things are asserted together because each of them can be
/// separately wrong: the resolve, the load, the SECOND require answering
/// `false`, and one `$LOADED_FEATURES` entry naming the library.
#[test]
fn a_computed_require_loads_a_compiled_extension() {
    if !have("make") || !have("cc") {
        eprintln!("skipping: this machine has no `make` or no `cc`");
        return;
    }
    let dir = extension_dir("require", "probe", PROBE_C);
    zeo::cext::configure(&zeo_bin(), &dir, Path::new("extconf.rb"), &[])
        .unwrap_or_else(|e| panic!("{e}"));
    let bundle = zeo::cext::build_extension(&dir, 4).unwrap_or_else(|e| panic!("{e}"));
    assert!(bundle.is_file(), "{} was not produced", bundle.display());

    let program = format!(
        r#"$LOAD_PATH.unshift({dir:?})
p $LOAD_PATH.resolve_feature_path("probe")[0]
f = "probe"
p require(f)
p probe_hi
p require(f)
p $LOADED_FEATURES.count {{ |e| e.end_with?({name:?}) }}
"#,
        dir = dir.display().to_string(),
        name = bundle
            .file_name()
            .and_then(|n| n.to_str())
            .expect("the bundle has a name"),
    );
    let out = Command::new(zeo_bin())
        .arg("-e")
        .arg(&program)
        .env("ZEO_CACHE", "0")
        .output()
        .expect("zeo runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        ":so\ntrue\n\"hi\"\nfalse\n1\n",
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The explicit-suffix spelling names the same library, and the loader
    // must not try to read it as source.
    let out = Command::new(zeo_bin())
        .arg("-e")
        .arg(format!(
            "$LOAD_PATH.unshift({:?})\np require(\"probe.{}\")\np probe_hi\n",
            dir.display().to_string(),
            bundle
                .extension()
                .and_then(|e| e.to_str())
                .expect("the bundle has a suffix"),
        ))
        .env("ZEO_CACHE", "0")
        .output()
        .expect("zeo runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "true\n\"hi\"\n",
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A C extension REOPENS a compiled namespace instead of minting a second one.
///
/// `rb_define_class_under(mod, "Engine", ...)` asked only the constant table,
/// and a compiled `module M; class Engine` is registered by its qualified NAME
/// instead -- codegen resolves `M::Engine` statically, so nothing ever
/// `const_set`s it. The extension therefore built a SECOND `Engine`, put its
/// rows there, and rebound the constant; the program's own `M::Engine`, folded
/// to the compiled id, had none of them.
///
/// bcrypt is the corpus case: `BCrypt::Engine.__bc_salt` is defined in C and
/// made private by the Ruby half, and reading `BCrypt.constants` was enough to
/// make it unreachable.
///
/// The identity assertion is the one that matters -- `equal?` was false, and
/// every other symptom followed from that.
#[test]
fn a_c_extension_reopens_a_compiled_namespace() {
    if !have("make") || !have("cc") {
        eprintln!("skipping: this machine has no `make` or no `cc`");
        return;
    }
    let dir = extension_dir("nested", "nested_probe", NESTED_PROBE_C);
    zeo::cext::configure(&zeo_bin(), &dir, Path::new("extconf.rb"), &[])
        .unwrap_or_else(|e| panic!("{e}"));
    zeo::cext::build_extension(&dir, 4).unwrap_or_else(|e| panic!("{e}"));

    let program = format!(
        r#"$LOAD_PATH.unshift({dir:?})
require "nested_probe"
module NestedProbe
  class Engine
    private_class_method :__np_salt
    def self.gen = __np_salt
  end
end
p NestedProbe::Engine.equal?(NestedProbe.const_get(:Engine))
p NestedProbe::Engine.respond_to?(:__np_salt, true)
p NestedProbe.constants
p NestedProbe::Engine.gen
p NestedProbe::Engine.respond_to?(:__np_salt)
"#,
        dir = dir.display().to_string(),
    );
    let out = Command::new(zeo_bin())
        .arg("-e")
        .arg(&program)
        .env("ZEO_CACHE", "0")
        .output()
        .expect("zeo runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "true\ntrue\n[:Engine]\n\"SALT\"\nfalse\n",
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// With `ZEO_DISABLE_BUILTIN`, a gem's own `module JSON` files reopen ONE
/// user class instead of each minting a fresh one.
///
/// The builtin's dormant slot registers first and the name index is
/// first-registered-wins, so the slot shadowed the name forever: every
/// definition site failed the not-provided filter and minted ANOTHER class,
/// scattering the gem's constants across them -- `JSON::NaN` was on a class
/// no read could reach, and on the store road the surviving binding sat in
/// an autoload unit that never ran, so `JSON` itself was invisible at run
/// time. A slot the build cannot provide is invisible to name resolution
/// now, which is also CRuby's world: no constant exists until a file
/// defines one.
#[test]
fn a_disabled_builtin_name_reopens_one_user_class_across_files() {
    let dir = std::env::temp_dir().join(format!("zeo-disable-reopen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    std::fs::write(dir.join("dv.rb"), "module JSON\n  X = 1\nend\n").expect("dv.rb");
    std::fs::write(
        dir.join("dc.rb"),
        "require \"dv\"\nmodule JSON\n  Y = 2\nend\n",
    )
    .expect("dc.rb");
    let out = Command::new(zeo_bin())
        .arg("-I")
        .arg(&dir)
        .arg("-e")
        .arg("require \"dc\"\np JSON::X\np JSON::Y\np defined?(JSON)\np JSON.constants.sort")
        .env("ZEO_CACHE", "0")
        .env("ZEO_DISABLE_BUILTIN", "json")
        .output()
        .expect("zeo runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1\n2\n\"constant\"\n[:X, :Y]\n",
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A shared object zeo did not build is refused BY NAME, on both loaders.
///
/// The load path is full of them: `bundle install` leaves the extension it
/// compiled for CRuby beside the gem's Ruby, and a runtime `require` used to
/// dlopen it -- machine code against CRuby's object layout, which ran until
/// its first field read and then faulted with no name for what went wrong.
/// Both the compile-time `-I` road (a literal require) and the runtime road
/// (a computed one) now answer a `LoadError` that names the file and the
/// reason, so a `rescue LoadError` takes the gem's own fallback exactly as
/// it would on a ruby without the extension.
#[test]
fn a_shared_object_zeo_did_not_build_is_refused_by_name() {
    let dir = std::env::temp_dir().join(format!("zeo-foreign-so-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let ext = if cfg!(target_os = "macos") {
        "bundle"
    } else {
        "so"
    };
    std::fs::write(dir.join(format!("nope.{ext}")), b"not a shared object")
        .expect("a fake product");
    let out = Command::new(zeo_bin())
        .arg("-I")
        .arg(&dir)
        .arg("-e")
        .arg(
            r#"begin
  require "nope"
rescue LoadError => e
  puts e.message.include?("was not built by zeo")
end
name = "no" + "pe"
begin
  require name
rescue LoadError => e
  puts e.message.include?("was not built by zeo")
end"#,
        )
        .env("ZEO_CACHE", "0")
        .output()
        .expect("zeo runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "true\ntrue\n",
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A raise is a Rust unwind through the extension's own frames.
///
/// `rb_raise` three C frames deep lands in the Ruby `rescue` above the
/// call; `rb_protect` catches its body's raise, answers a non-zero state and
/// leaves the exception in `rb_errinfo`. Both need the extension's objects
/// to carry unwind tables, which is why the Makefile's compile line is
/// asserted too: without `-fexceptions -fasynchronous-unwind-tables` the
/// unwind cannot cross those frames.
#[test]
fn a_raise_unwinds_the_extensions_own_frames() {
    if !have("cc") {
        eprintln!("skipping: this machine has no `cc`");
        return;
    }
    let dir = std::env::temp_dir().join(format!("zeo-cext-unwind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    std::fs::write(
        dir.join("extconf.rb"),
        "require \"mkmf\"\n$CFLAGS << \" -fno-exceptions\"\ncreate_makefile(\"unwind_probe\")\n",
    )
    .expect("write extconf.rb");
    std::fs::write(
        dir.join("unwind_probe.c"),
        r#"#include <ruby.h>

static VALUE deep3(VALUE self) { rb_raise(rb_eArgError, "three frames deep"); }
static VALUE deep2(VALUE self) { return deep3(self); }
static VALUE deep1(VALUE self) { return deep2(self); }

static VALUE body(VALUE arg) { rb_raise(rb_eRuntimeError, "inside rb_protect"); }

static VALUE protected_state(VALUE self)
{
    int state = 0;
    VALUE r = rb_protect(body, Qnil, &state);
    VALUE err = rb_errinfo();
    rb_set_errinfo(Qnil);
    return rb_ary_new_from_args(3, INT2NUM(state), r, err);
}

void Init_unwind_probe(void)
{
    VALUE m = rb_define_module("UnwindProbe");
    rb_define_singleton_method(m, "deep", deep1, 0);
    rb_define_singleton_method(m, "protected_state", protected_state, 0);
}
"#,
    )
    .expect("write unwind_probe.c");
    zeo::cext::configure(&zeo_bin(), &dir, Path::new("extconf.rb"), &[])
        .unwrap_or_else(|e| panic!("{e}"));
    let makefile = std::fs::read_to_string(dir.join("Makefile")).expect("mkmf wrote a Makefile");
    let cflags = makefile
        .lines()
        .find(|l| l.starts_with("CFLAGS"))
        .expect("the Makefile has a CFLAGS line");
    assert!(
        cflags.contains("-fexceptions") && cflags.contains("-fasynchronous-unwind-tables"),
        "the compile line carries no unwind tables: {cflags}"
    );
    assert!(
        !cflags.contains("-fno-exceptions"),
        "the gem's -fno-exceptions survived mkmf: {cflags}"
    );
    zeo::cext::build_extension(&dir, 4).unwrap_or_else(|e| panic!("{e}"));

    let program = format!(
        r#"$LOAD_PATH.unshift({dir:?})
require "unwind_probe"
begin
  UnwindProbe.deep
rescue ArgumentError => e
  p e.message
end
state, r, err = UnwindProbe.protected_state
p state != 0, r, err.class, err.message
"#,
        dir = dir.display().to_string(),
    );
    let out = Command::new(zeo_bin())
        .arg("-e")
        .arg(&program)
        .env("ZEO_CACHE", "0")
        .output()
        .expect("zeo runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "\"three frames deep\"\ntrue\nnil\nRuntimeError\n\"inside rb_protect\"\n",
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The GVL is a latch, not a load-time cost. A single-threaded program that
/// loads a C extension keeps its lock-free path, and the second Ruby thread
/// arms the GVL -- spawned after the load, or running before it. The latch's
/// own log line is the instrument, the same constant the C-gem sweep and the
/// pure-stdlib test watch for.
#[test]
fn a_c_extension_arms_the_gvl_only_with_a_second_thread() {
    if !have("cc") {
        eprintln!("skipping: this machine has no `cc`");
        return;
    }
    let dir = std::env::temp_dir().join(format!("zeo-cext-latch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    std::fs::write(
        dir.join("extconf.rb"),
        "require \"mkmf\"\ncreate_makefile(\"latch_probe\")\n",
    )
    .expect("write extconf.rb");
    std::fs::write(
        dir.join("latch_probe.c"),
        r#"#include <ruby.h>

static VALUE answer(VALUE self) { return INT2NUM(42); }

void Init_latch_probe(void)
{
    VALUE m = rb_define_module("LatchProbe");
    rb_define_singleton_method(m, "answer", answer, 0);
}
"#,
    )
    .expect("write latch_probe.c");
    zeo::cext::configure(&zeo_bin(), &dir, Path::new("extconf.rb"), &[])
        .unwrap_or_else(|e| panic!("{e}"));
    zeo::cext::build_extension(&dir, 4).unwrap_or_else(|e| panic!("{e}"));

    let prelude = format!("$LOAD_PATH.unshift({:?})\n", dir.display().to_string());
    let run = |body: &str| {
        let out = Command::new(zeo_bin())
            .arg("-e")
            .arg(format!("{prelude}{body}"))
            .env("ZEO_CACHE", "0")
            .env("ZEO_LOG", "zeo_rt::concurrency::gvl=debug")
            .output()
            .expect("zeo runs");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "42\n",
            "program:\n{body}\nstderr:\n{stderr}"
        );
        stderr.contains(zeo_rt::gvl::CEXT_ARMED_SENTINEL)
    };

    assert!(
        !run("require \"latch_probe\"\np LatchProbe.answer\n"),
        "a single-threaded program armed the GVL"
    );
    assert!(
        run("require \"latch_probe\"\np Thread.new { LatchProbe.answer }.value\n"),
        "a thread spawned after the load did not arm the GVL"
    );
    assert!(
        run("Thread.new { 1 }.join\nrequire \"latch_probe\"\np LatchProbe.answer\n"),
        "a load after a thread did not arm the GVL"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Every accessor the header edits redirect answers through a view, end to
/// end: a write through `RSTRING_PTR` reaches the String, `RARRAY_ASET`
/// reaches the Array, `ROBJECT_FIELDS` reads the ivars, `RTYPEDDATA(o)->data`
/// is the object's own slot, `RFILE(io)->fptr->fd` is the descriptor,
/// `RMATCH_REGS` are the match's offsets, and the encoding index and
/// coderange are the String's own.
#[test]
fn every_edited_accessor_answers_through_its_view() {
    if !have("cc") {
        eprintln!("skipping: this machine has no `cc`");
        return;
    }
    let dir = std::env::temp_dir().join(format!("zeo-cext-views-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    std::fs::write(
        dir.join("extconf.rb"),
        "require \"mkmf\"\ncreate_makefile(\"view_probe\")\n",
    )
    .expect("write extconf.rb");
    std::fs::write(
        dir.join("view_probe.c"),
        r#"#include <ruby.h>
#include <ruby/encoding.h>
#include <ruby/io.h>
#include <ruby/re.h>

struct cell { long n; };
static void cell_free(void *p) { ruby_xfree(p); }
static const rb_data_type_t cell_type = {
    "zeo/view_probe/cell", { NULL, cell_free, NULL, NULL, { NULL } }, 0, 0, RUBY_TYPED_FREE_IMMEDIATELY,
};

static VALUE upcase_first(VALUE self, VALUE s)
{
    char *p = RSTRING_PTR(s);
    if (RSTRING_LEN(s) > 0 && p[0] >= 'a' && p[0] <= 'z') p[0] -= 32;
    return LONG2NUM(RSTRING_LEN(s));
}

static VALUE swap_ends(VALUE self, VALUE a)
{
    long n = RARRAY_LEN(a);
    VALUE first = RARRAY_AREF(a, 0);
    RARRAY_ASET(a, 0, RARRAY_AREF(a, n - 1));
    RARRAY_ASET(a, n - 1, first);
    return a;
}

static VALUE first_ivar(VALUE self, VALUE o)
{
    return ROBJECT_FIELDS(o)[0];
}

static VALUE cell_new(VALUE klass, VALUE n)
{
    struct cell *c = ruby_xmalloc(sizeof *c);
    c->n = NUM2LONG(n);
    VALUE o = TypedData_Wrap_Struct(klass, &cell_type, c);
    struct cell *again = ruby_xmalloc(sizeof *c);
    again->n = c->n * 2;
    ruby_xfree(RTYPEDDATA(o)->data);
    RTYPEDDATA(o)->data = again;
    return o;
}

static VALUE cell_n(VALUE self)
{
    struct cell *c;
    TypedData_Get_Struct(self, struct cell, &cell_type, c);
    return LONG2NUM(c->n);
}

static VALUE fd_of(VALUE self, VALUE io)
{
    rb_io_t *fptr;
    GetOpenFile(io, fptr);
    return INT2NUM(RFILE(io)->fptr->fd == fptr->fd ? fptr->fd : -1);
}

static VALUE match_span(VALUE self, VALUE m)
{
    struct re_registers *regs = RMATCH_REGS(m);
    return rb_ary_new_from_args(2, LONG2NUM(regs->beg[1]), LONG2NUM(regs->end[1]));
}

static VALUE enc_facts(VALUE self, VALUE s)
{
    int cr = RB_ENC_CODERANGE(s);
    RB_ENC_CODERANGE_SET(s, RUBY_ENC_CODERANGE_7BIT);
    return rb_ary_new_from_args(3,
        INT2NUM(ENCODING_GET(s) == rb_utf8_encindex() ? 8 : ENCODING_GET(s) == rb_ascii8bit_encindex() ? 0 : -1),
        INT2NUM(cr == RUBY_ENC_CODERANGE_7BIT ? 7 : cr == RUBY_ENC_CODERANGE_VALID ? 1 : 0),
        INT2NUM(RB_ENC_CODERANGE(s) == cr));
}

void Init_view_probe(void)
{
    VALUE m = rb_define_module("ViewProbe");
    rb_define_singleton_method(m, "upcase_first", upcase_first, 1);
    rb_define_singleton_method(m, "swap_ends", swap_ends, 1);
    rb_define_singleton_method(m, "first_ivar", first_ivar, 1);
    rb_define_singleton_method(m, "fd_of", fd_of, 1);
    rb_define_singleton_method(m, "match_span", match_span, 1);
    rb_define_singleton_method(m, "enc_facts", enc_facts, 1);
    VALUE c = rb_define_class_under(m, "Cell", rb_cObject);
    rb_undef_alloc_func(c);
    rb_define_singleton_method(c, "new", cell_new, 1);
    rb_define_method(c, "n", cell_n, 0);
}
"#,
    )
    .expect("write view_probe.c");
    zeo::cext::configure(&zeo_bin(), &dir, Path::new("extconf.rb"), &[])
        .unwrap_or_else(|e| panic!("{e}"));
    zeo::cext::build_extension(&dir, 4).unwrap_or_else(|e| panic!("{e}"));

    let program = format!(
        r#"$LOAD_PATH.unshift({dir:?})
require "view_probe"
s = +"hello"
p [ViewProbe.upcase_first(s), s]
p ViewProbe.swap_ends([1, 2, 3])
class Box; def initialize; @first = :one; @second = :two; end; end
p ViewProbe.first_ivar(Box.new)
p ViewProbe::Cell.new(21).n
File.open(File.join({dir:?}, "extconf.rb")) {{ |f| p ViewProbe.fd_of(f) == f.fileno }}
p ViewProbe.match_span("xxabcxx".match(/x(abc)x/))
p ViewProbe.enc_facts("plain")
p ViewProbe.enc_facts("café")
p ViewProbe.enc_facts("café".b)
"#,
        dir = dir.display().to_string(),
    );
    let out = Command::new(zeo_bin())
        .arg("-e")
        .arg(&program)
        .env("ZEO_CACHE", "0")
        .output()
        .expect("zeo runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "[5, \"Hello\"]\n[3, 2, 1]\n:one\n42\ntrue\n[2, 5]
[8, 7, 1]
\
         [8, 1, 1]
[0, 1, 1]
",
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}
