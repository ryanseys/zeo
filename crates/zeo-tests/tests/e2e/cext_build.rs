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
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repo root is reachable from the manifest dir")
}

/// The `zeo` binary the harness spawns elsewhere. `cargo build --workspace`
/// is what produces it; a missing one is a stale tree, not a skip.
fn zeo_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("ZEO_BIN") {
        return PathBuf::from(p);
    }
    let root = repo_root();
    for profile in ["debug", "release"] {
        let p = root.join("target").join(profile).join("zeo");
        if p.is_file() {
            return p;
        }
    }
    panic!("no `zeo` binary under target/{{debug,release}} -- run `cargo build --workspace`");
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

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
