//! Building a gem's C extension.
//!
//! An extension arrives as `ext/**/*.c` plus an `extconf.rb`. Three steps
//! turn that into something loadable, and zeo owns all three:
//!
//! 1. Run `extconf.rb` under zeo, with `RbConfig::CONFIG` supplied by
//!    `crates/zeo/src/parse/shims/rbconfig.rb.in` and `mkmf` vendored from
//!    the same `ruby/ruby` pin as the headers. It writes a Makefile.
//! 2. Read that Makefile's variables ([`makefile`]) and compile and link
//!    from them ([`build`]), in parallel and without `make`.
//! 3. Hand the shared object to the loader.
//!
//! Step 2 refuses any Makefile carrying a rule mkmf did not write and runs
//! real `make` instead -- see [`build`] for why that is the right way round.

pub mod build;
pub mod makefile;

use crate::home::ZeoHome;
use std::path::{Path, PathBuf};

/// Where the vendored MRI headers landed, as `(include, config)`.
///
/// This is a RUN-TIME fact. `crates/zeo/build.rs` bakes the dev tree's paths
/// into the rbconfig shim as a fallback, and an installed zeo's are under its
/// payload -- so a released binary that used the compile-time constant would
/// point every extension build at the build machine's source tree.
pub fn header_dirs() -> Result<(PathBuf, PathBuf), String> {
    let root = match crate::home::zeo_home() {
        ZeoHome::DevTree { root } => root.join("crates/zeo-rt/cext"),
        ZeoHome::Installed { payload, .. } => payload.join("cext"),
        // A `cargo install`ed zeo has no payload directory at all, so the
        // headers ride nowhere it can reach. Saying so beats handing an
        // extension an include path that does not exist.
        ZeoHome::Registry { .. } => {
            return Err("a cargo-installed zeo carries no C extension headers; \
                        install the release tarball to build native gems"
                .into());
        }
    };
    let (include, config) = (root.join("include"), root.join("config"));
    if !include.join("ruby.h").is_file() {
        return Err(format!(
            "the C extension headers are missing from {} -- this install is incomplete",
            root.display()
        ));
    }
    Ok((include, config))
}

/// Run `extconf.rb` in `dir` with `zeo`, so mkmf writes a Makefile.
///
/// zeo re-enters itself as a SUBPROCESS rather than compiling the file in
/// place, for the same reason rubygems does: an `extconf.rb` calls `exit`
/// freely, writes into the current directory, and installs constants and
/// globals a long-running process would then carry. A child gets its own of
/// each, and its exit status is the answer.
///
/// `zeo` is passed rather than read from `current_exe`, because the caller is
/// not always the binary -- a test is the harness, and spawning that with an
/// `extconf.rb` would run the test suite instead.
pub fn configure(zeo: &Path, dir: &Path, extconf: &Path, args: &[String]) -> Result<(), String> {
    let (include, config) = header_dirs()?;
    let out = std::process::Command::new(zeo)
        .arg(extconf)
        .args(args)
        .current_dir(dir)
        // The rbconfig shim reads these; without them it falls back to the
        // paths `build.rs` baked in, which only exist in the dev tree.
        .env("ZEO_CEXT_HDRDIR", &include)
        .env("ZEO_CEXT_ARCHHDRDIR", &config)
        .output()
        .map_err(|e| format!("running {}: {e}", extconf.display()))?;
    if out.status.success() && dir.join("Makefile").is_file() {
        return Ok(());
    }
    // extconf's own output is the whole diagnosis -- a `have_library` that
    // failed says which library, and nothing else can.
    Err(format!(
        "{} did not produce a Makefile\n{}{}",
        extconf.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// Build the extension whose Makefile is in `dir`, and answer the shared
/// object.
///
/// `jobs` bounds the parallel compiles. The `make` fallback gets the same
/// number through `-j`.
pub fn build_extension(dir: &Path, jobs: usize) -> Result<PathBuf, String> {
    let plan =
        build::Plan::of(dir).map_err(|e| format!("reading {}/Makefile: {e}", dir.display()))?;
    match plan {
        Ok(plan) => plan.run(jobs).map_err(|e| e.to_string()),
        Err(why) => {
            tracing::debug!("cext: running make in {} -- {why}", dir.display());
            build::run_make(dir, jobs).map_err(|e| e.to_string())?;
            product_of(dir).ok_or_else(|| {
                format!(
                    "make ran in {} and produced no shared object",
                    dir.display()
                )
            })
        }
    }
}

/// The shared object a `make` run left behind. The Makefile names it in
/// `TARGET_SO`, so that is what is read rather than a directory scan -- a
/// scan would pick up a `.bundle` from an earlier build of a different gem.
fn product_of(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dir.join("Makefile")).ok()?;
    let name = makefile::Makefile::parse(&text).get("TARGET_SO");
    let path = dir.join(name.trim());
    path.is_file().then_some(path)
}
