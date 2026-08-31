//! A C extension configures and builds under zeo, end to end.
//!
//! This is the only test that exercises the whole build pipeline at once:
//! `extconf.rb` runs under zeo, which means `mkmf` loads and probes; mkmf
//! writes a Makefile; `make` compiles the C against the vendored MRI headers;
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

fn repo_root() -> PathBuf {
    crate::paths::workspace_root()
        .canonicalize()
        .expect("the repo root is reachable from the manifest dir")
}

/// The `zeo` binary beside this test binary's profile dir -- unlike the
/// old `target/{debug,release}` guess, this survives CARGO_TARGET_DIR and
/// custom profiles.
fn zeo_bin() -> PathBuf {
    crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"))
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `RbConfig.ruby` names the zeo that is running, and that file exists.
///
/// rubygems spawns exactly this string to run a gem's `extconf.rb`
/// (`Gem.ruby` is `RbConfig.ruby`). The shim used to synthesize
/// `/usr/local/bin/ruby` from a hardcoded FHS prefix, so every gem with a C
/// extension died at `extconf failed: No such file or directory` while
/// pure-ruby gems installed fine -- a failure that names a path nobody in the
/// repo ever wrote.
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

/// FLAKY under a loaded machine, observed 2026-08-31: one full `-p zeo` run
/// failed here with "extconf.rb did not produce a Makefile" after 1.5s, where
/// a passing run takes 8-19s. It passes on its own every time, and the same
/// run's other 6,104 tests passed. Cause NOT established -- note the scratch
/// directory is keyed on the pid alone, which is the shape of the collision
/// that caused this repo's long-standing one-random-failure-per-run flake
/// before, so that is where to look first. Re-run before believing a failure
/// here names a real defect.
#[test]
fn an_extension_configures_compiles_and_links() {
    // `make` and a C compiler are what an extension build IS. A machine
    // without them cannot run this, and saying so beats a confusing failure.
    if !have("make") || !have("cc") {
        eprintln!("skipping: this machine has no `make` or no `cc`");
        return;
    }

    let root = repo_root();
    let dir = std::env::temp_dir().join(format!("zeo-cext-build-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for f in ["extconf.rb", "probe.c"] {
        std::fs::copy(root.join("tests/cext_probe").join(f), dir.join(f))
            .unwrap_or_else(|e| panic!("copying {f}: {e}"));
    }

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
/// The literal spelling has always worked: the compile-time loader maps the
/// feature onto a store gem, builds it, and splices a `CExtLoaded` node. A
/// computed one names a feature no compile can see, so it has to resolve and
/// dlopen at RUN time -- and the run-time resolver used to be Ruby-source
/// only, trying `.rb` and the verbatim name and reading every hit as text.
/// `%w[...].each { |f| require f }` over a native name raised `LoadError`,
/// and the explicit `require "probe.bundle"` failed with "stream did not
/// contain valid UTF-8", which reads like a corrupt file rather than a
/// loader that took the wrong branch.
///
/// Four things are asserted together because each of them was separately
/// wrong: the resolve, the load, the SECOND require answering `false`, and
/// one `$LOADED_FEATURES` entry naming the library.
#[test]
fn a_computed_require_loads_a_compiled_extension() {
    if !have("make") || !have("cc") {
        eprintln!("skipping: this machine has no `make` or no `cc`");
        return;
    }
    let root = repo_root();
    let dir = std::env::temp_dir().join(format!("zeo-cext-require-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for f in ["extconf.rb", "probe.c"] {
        std::fs::copy(root.join("tests/cext_probe").join(f), dir.join(f))
            .unwrap_or_else(|e| panic!("copying {f}: {e}"));
    }
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
    let root = repo_root();
    let dir = std::env::temp_dir().join(format!("zeo-cext-nested-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for f in ["extconf.rb", "nested_probe.c"] {
        std::fs::copy(root.join("tests/cext_probe_nested").join(f), dir.join(f))
            .unwrap_or_else(|e| panic!("copying {f}: {e}"));
    }
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
