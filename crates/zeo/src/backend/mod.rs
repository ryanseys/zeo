//! Turns a compiled program into something that runs.
//!
//! Three backends live here, dispatched by [`Backend`]. [`jit`] finalizes
//! Cranelift output into this process and runs it in place -- the default
//! for run mode. [`object`] + [`link`] write a Cranelift object file and
//! link it against `libzeo.a` -- the default for `-o`/`--compile`, and what
//! ships. [`build_binary`] is the third: the frozen rustc emitter's path,
//! kept selectable as `--backend rustc` so a suspect CLIF result can be
//! diffed against the emitter that was correct before it (retired at M3).
//!
//! The rustc path is DEV-TREE ONLY and deliberately unassisted. It runs one
//! `rustc` against the `zeo-rt` rlib the workspace's own `cargo build`
//! produced, and does nothing else: it does not build the runtime, shell
//! cargo, consult a cache, or write anywhere but its output. Everything that
//! once made it fast for a whole-corpus sweep -- the content-keyed binary
//! cache, the shared runtime dylib, the per-variant target dirs, the build
//! lock -- is gone, because that sweep is no longer part of any gate. What
//! the oracle is actually for is rerunning ONE golden, which costs one
//! `rustc` invocation.
//!
//! Two things guard the identity of what comes out. `--crate-name` is pinned
//! and the temp source is content-addressed, because `rustc` otherwise
//! derives the crate name from the source FILENAME and embeds that path as
//! panic-location metadata -- either one makes identical Ruby compile to
//! different bytes.
//!
//! Only ONE `--extern` is ever needed. `zeo-rt`'s transitive dependencies are
//! deliberately not enumerated by hand: the generated program references
//! `zeo_rt` alone, and `rustc` resolves the rest through the `-L` search path
//! from metadata embedded in the built `zeo-rt`. This is the same shape
//! `trybuild`/`compiletest` use.

pub mod jit;
pub mod link;
pub mod object;

use std::path::{Path, PathBuf};

/// The pinned crate name for every generated program.
///
/// Without it `rustc` derives the crate name from the source FILENAME, which
/// is content-addressed and therefore varies -- and it embeds that name in
/// panic-location metadata, so identical Ruby would compile to different
/// bytes.
const GENERATED_CRATE_NAME: &str = "zeo_program";

/// Which cargo profile's `zeo-rt` a generated program links against.
///
/// Passed explicitly rather than read from the environment down here: the
/// e2e harness calls this from a dozen `#[test]` threads at once, and a
/// `set_var` racing a `var_os` is a real data race. The CLI reads the
/// environment once, at startup, while still single-threaded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    /// The caller's default, overridable by `ZEO_RUNTIME_PROFILE`
    /// (`debug`/`release`). An unrecognized value keeps the default.
    pub fn from_env_or(default: Profile) -> Self {
        match std::env::var_os("ZEO_RUNTIME_PROFILE")
            .as_deref()
            .and_then(|s| s.to_str())
        {
            Some("release") => Profile::Release,
            Some("debug") => Profile::Debug,
            _ => default,
        }
    }

    /// The `target/` subdirectory cargo writes this profile's artifacts to.
    fn subdir(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }

    /// Extra `rustc` flags for the GENERATED crate when it follows the
    /// profile (`GenOpt::Optimized`): Release optimizes the generated code,
    /// not just the linked runtime rlib -- the generated main is where typed
    /// fast paths (inline Int arithmetic, direct calls) live, and at `-O0`
    /// those and every cross-crate `#[inline]` hint stay unoptimized calls.
    ///
    /// BOTH strip symbols. Most of a statically-linked program's size is the
    /// `zeo-rt` rlib's debug info, which is worthless here: zeo stamps its
    /// OWN Ruby backtraces and never relies on Rust-level symbols.
    fn rustc_flags(self) -> &'static [&'static str] {
        match self {
            // `-O0` frames are an order of magnitude fatter than optimized
            // ones, so a Debug binary gets a roomier main stack (64MB vs the
            // 8MB default): deep Ruby recursion that comfortably fits
            // optimized frames (the corpus's 2000-deep returning-proc chain)
            // overflows the default at `-O0`.
            Profile::Debug if cfg!(target_os = "macos") => &[
                "-C",
                "strip=symbols",
                "-C",
                "link-arg=-Wl,-stack_size,0x4000000",
            ],
            Profile::Debug => &["-C", "strip=symbols"],
            Profile::Release => &["-C", "opt-level=2", "-C", "strip=symbols"],
        }
    }
}

/// Which optimization level the GENERATED crate itself is compiled at --
/// deliberately decoupled from `Profile`, which selects the runtime rlib.
/// `-O2` over a gem-scale generated main costs `rustc` orders of magnitude
/// more time for no output difference, so only a shipped artifact asks for
/// it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GenOpt {
    Optimized,
    Unoptimized,
}

impl GenOpt {
    fn rustc_flags(self, profile: Profile) -> &'static [&'static str] {
        match self {
            GenOpt::Optimized => profile.rustc_flags(),
            GenOpt::Unoptimized => Profile::Debug.rustc_flags(),
        }
    }
}

/// The workspace's own `target/<profile>/deps`, where cargo leaves the
/// `zeo-rt` rlib every ordinary build produces.
///
/// The `zeo` lib depends on `zeo-rt`, so `cargo build` cannot leave it
/// missing -- which is what lets this path be a pure lookup with no build
/// step behind it. `CARGO_TARGET_DIR` is honoured because cargo honours it.
fn deps_dir(profile: Profile) -> Result<PathBuf, String> {
    let root = match crate::home::zeo_home() {
        crate::home::ZeoHome::DevTree { root } => root.clone(),
        // An installed or `cargo install`ed zeo has no cargo target dir and
        // no rustc contract to keep. It ships `libzeo.a` beside the binary
        // and uses the Cranelift backends, which is the whole shipped story.
        _ => {
            return Err(
                "`--backend rustc` needs the zeo repo's own cargo build tree; \
                 an installed zeo uses the Cranelift backends (drop `--backend rustc`)"
                    .to_string(),
            );
        }
    };
    let target = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => root.join("target"),
    };
    Ok(target.join(profile.subdir()).join("deps"))
}

/// The `zeo-rt` rlib a generated program links against.
///
/// Cargo stamps a metadata hash into the file name, so the artifact is found
/// by scanning rather than by a fixed path; a tree that somehow holds several
/// takes the newest, which is the one the current build wrote.
fn runtime_rlib(profile: Profile) -> Result<PathBuf, String> {
    let deps = deps_dir(profile)?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let entries = std::fs::read_dir(&deps)
        .map_err(|e| format!("reading {} (run `cargo build`): {e}", deps.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("libzeo_rt-") || !name.ends_with(".rlib") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(best, _)| mtime > *best) {
            best = Some((mtime, entry.path()));
        }
    }
    best.map(|(_, path)| path).ok_or_else(|| {
        format!(
            "zeo-rt is not built: no libzeo_rt-*.rlib in {} -- run `cargo build{}`",
            deps.display(),
            match profile {
                Profile::Debug => "",
                Profile::Release => " --release",
            }
        )
    })
}

/// Compiles `rust_source` to `output` with one `rustc` invocation.
///
/// PURE: it links an already-built `zeo-rt` and errors if the artifact is
/// missing, never running cargo or mutating the workspace, so parallel `zeo`
/// subprocesses never contend on cargo's exclusive build-directory lock.
pub fn build_binary(
    rust_source: &str,
    output: &Path,
    profile: Profile,
    gen_opt: GenOpt,
) -> Result<(), String> {
    let runtime_lib = runtime_rlib(profile)?;
    let deps = deps_dir(profile)?;

    let src_path = std::env::temp_dir().join(format!(
        "{GENERATED_CRATE_NAME}-{:016x}.rs",
        fnv1a64(rust_source.as_bytes())
    ));
    std::fs::write(&src_path, rust_source)
        .map_err(|e| format!("writing {}: {e}", src_path.display()))?;

    let mut cmd = std::process::Command::new("rustc");
    cmd.arg("--edition")
        .arg("2021")
        .arg("--crate-name")
        .arg(GENERATED_CRATE_NAME)
        .arg(&src_path)
        .arg("-o")
        .arg(output)
        .arg("--extern")
        .arg(format!("zeo_rt={}", runtime_lib.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps.display()));
    cmd.args(gen_opt.rustc_flags(profile));
    // Arms the generated crate's mimalloc `#[global_allocator]` (see codegen's
    // main assembly): a self-contained binary owns every allocation, so the
    // swap is safe there. Every rustc-backed binary is self-contained now --
    // the dynamic-linkage variant went with the bin cache it existed for.
    cmd.arg("--cfg").arg("zeo_static_alloc");

    let t_rustc = std::time::Instant::now();
    let status = cmd.status().map_err(|e| format!("running rustc: {e}"))?;
    if !status.success() {
        // The build path skips the in-compiler syn validation; re-parse here
        // so a codegen token bug still reports as a zeo bug, not a bare
        // rustc error in a temp file.
        let triage = match syn::parse_file(rust_source) {
            Err(e) => format!(" (generated Rust fails to parse -- a zeo bug: {e})"),
            Ok(_) => String::new(),
        };
        return Err(format!(
            "rustc failed compiling the generated program (source at {}){triage}",
            src_path.display()
        ));
    }
    if crate::timings_enabled() {
        let bytes = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
        eprintln!(
            "zeo-timings: rustc={}ms bin_bytes={bytes}",
            t_rustc.elapsed().as_millis()
        );
    }
    let _ = std::fs::remove_file(&src_path);
    Ok(())
}

/// FNV-1a over `bytes` -- names the temp source after its own content, so
/// two concurrent builds of the same program cannot write different bytes to
/// one path.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Which code-generation backend turns a compiled program into machine code.
///
/// `Rustc`: emitted Rust text -> `rustc` -> binary, linking the prebuilt
/// `zeo-rt` artifact -- the default during the dual period. `Aot`: the
/// Cranelift path -- HIR -> CLIF -> object file (`clif/`), linked against
/// `libzeo.a` (`link.rs`). `Jit`: the same CLIF finalized into THIS
/// process's memory and run in place (`jit.rs`) -- run mode only, no
/// artifact.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Rustc,
    Aot,
    Jit,
}

impl Backend {
    /// A `--backend`/`ZEO_BACKEND` value.
    pub fn parse(value: &str) -> Result<Backend, String> {
        match value {
            "rustc" => Ok(Backend::Rustc),
            "aot" => Ok(Backend::Aot),
            "jit" => Ok(Backend::Jit),
            other => Err(format!(
                "unknown backend `{other}` (expected rustc, aot, or jit)"
            )),
        }
    }

    /// The backend this invocation uses: the CLI flag, else `ZEO_BACKEND`,
    /// else the default for the MODE. Cranelift is the default backend, and
    /// the two Cranelift modes are not interchangeable: the JIT runs a
    /// program in place, and only the AOT backend produces the artifact
    /// `-o`/`--compile` asks for. `--backend rustc` stays selectable as the
    /// differential oracle until M3.
    pub fn select(cli: Option<Backend>, wants_artifact: bool) -> Result<Backend, String> {
        if let Some(backend) = cli {
            return Ok(backend);
        }
        match std::env::var("ZEO_BACKEND") {
            Ok(value) if !value.is_empty() => Backend::parse(&value),
            Ok(_) | Err(_) => Ok(if wants_artifact {
                Backend::Aot
            } else {
                Backend::Jit
            }),
        }
    }
}

/// A compiled program in whichever form its backend produced -- the input
/// the two mode entries below dispatch on.
pub enum CompiledProgram<'a> {
    Rustc(&'a crate::CompileOutput),
    Aot(&'a crate::ObjectOutput),
}

/// Run mode (`zeo file.rb`, `zeo -e`): produce a throwaway program for
/// `compiled`, run it with `program_args`, and exit this process with the
/// program's status. Never returns on success.
///
/// Rustc: compile to a temp binary and exec it, against the fast-to-build
/// `Debug` runtime (`ZEO_RUNTIME_PROFILE` overrides).
pub fn run_program(
    compiled: &CompiledProgram<'_>,
    program_args: &[String],
) -> Result<std::convert::Infallible, String> {
    match compiled {
        CompiledProgram::Rustc(compiled) => {
            let profile = Profile::from_env_or(Profile::Debug);
            let bin = std::env::temp_dir().join(format!("zeo-e-{}", std::process::id()));
            build_binary(&compiled.rust_source, &bin, profile, GenOpt::Optimized)?;
            let status = std::process::Command::new(&bin)
                .args(program_args)
                .status()
                .map_err(|e| format!("running compiled program: {e}"))?;
            let _ = std::fs::remove_file(&bin);
            std::process::exit(status.code().unwrap_or(1));
        }
        // Aot run mode links a throwaway binary and runs it -- the
        // in-process JIT replaces this at M0.5.
        CompiledProgram::Aot(compiled) => {
            let bin = std::env::temp_dir().join(format!("zeo-e-{}", std::process::id()));
            object::object_to_binary(&compiled.object, compiled.debuginfo, &bin)?;
            let status = std::process::Command::new(&bin)
                .args(program_args)
                .status()
                .map_err(|e| format!("running compiled program: {e}"))?;
            let _ = std::fs::remove_file(&bin);
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}

/// Artifact mode (`zeo file.rb -o app`, `--compile`): produce the SHIPPED
/// binary at `output` for `compiled`.
///
/// Rustc: self-contained, against the release-profiled runtime (optimized +
/// stripped); `ZEO_RUNTIME_PROFILE` overrides (e.g. to symbolicate a runtime
/// panic).
pub fn build_artifact(compiled: &CompiledProgram<'_>, output: &Path) -> Result<(), String> {
    match compiled {
        CompiledProgram::Rustc(compiled) => build_binary(
            &compiled.rust_source,
            output,
            Profile::from_env_or(Profile::Release),
            GenOpt::Optimized,
        ),
        CompiledProgram::Aot(compiled) => {
            object::object_to_binary(&compiled.object, compiled.debuginfo, output)
        }
    }
}
