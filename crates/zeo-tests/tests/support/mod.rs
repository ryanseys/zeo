//! In-process test harness: compiles Ruby source through the DEFAULT
//! backend (Cranelift, to an object file) directly -- no subprocess spawn
//! for the compiler itself -- links a throwaway binary and runs it,
//! capturing stdout/stderr/exit status separately so tests can assert on
//! each independently. This is the default tier for zeo test coverage;
//! `tests/*.rb` + `.expected` remains as the "real CLI + real `ruby`-oracle"
//! golden suite.
//!
//! The compile-only negative-path checks scattered through `e2e/` still ask
//! `compile_to_rust*`: they assert front-end and loader POLICY (a `require`
//! that must not resolve, a form that must be rejected), which both backends
//! share, and the rustc emitter stays reachable until M3. Nothing here
//! BUILDS through rustc any more.

pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub status: std::process::ExitStatus,
}

/// Compile `source` with `opts` through the default backend, link the
/// object into a throwaway binary, run it with `env`/`args`, and hand back
/// what it printed. The one place the e2e tier builds a program.
pub fn compile_link_run(
    source: &str,
    opts: &zeo::CompileOptions,
    env: &[(&str, &str)],
    args: &[&str],
) -> RunResult {
    let compiled = zeo::compile_to_object_with(source, opts, false)
        .unwrap_or_else(|e| panic!("compile_to_object_with failed: {e}"));
    let bin = std::env::temp_dir().join(format!(
        "zeo-test-bin-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    zeo::backend::build_artifact(&zeo::backend::CompiledProgram::Aot(&compiled), &bin)
        .unwrap_or_else(|e| panic!("linking the test binary failed: {e}"));
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("running compiled binary: {e}"));
    let _ = std::fs::remove_file(&bin);
    RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        status: out.status,
    }
}

pub fn run_ruby(source: &str) -> RunResult {
    run_ruby_configured(source, &[], &[])
}

/// [`run_ruby`] under CRuby's own box gate. `RUBY_BOX=1` is a RUN-TIME
/// flag both ruby and zeo read, so a program that allocates a box is run
/// under it -- and one that pins the DISABLED shape deliberately is not.
#[allow(dead_code)] // each test binary compiles its own copy of this module
pub fn run_ruby_boxed(source: &str) -> RunResult {
    run_ruby_configured(source, &[("RUBY_BOX", "1")], &[])
}

/// [`run_ruby_project`] under the box gate -- see [`run_ruby_boxed`].
#[allow(dead_code)]
pub fn run_ruby_project_boxed(files: &[(&str, &str)], entry: &str, roots: &[&str]) -> RunResult {
    let (_dir, source, opts) = write_project(files, entry, roots, &[]);
    compile_link_run(&source, &opts, &[("RUBY_BOX", "1")], &[])
}

/// Multi-file harness for compile-time `require` resolution:
/// writes `files` (relative path -> source) into a fresh per-test temp
/// project directory, compiles `entry` (a key in `files`) with the project
/// dir itself as the requiring-file base and `roots` (relative to the
/// project dir) as `-I` search roots, then builds and runs it like
/// `run_ruby`. The temp tree is removed afterwards.
#[allow(dead_code)] // each test binary compiles its own copy of this module
pub fn run_ruby_project(files: &[(&str, &str)], entry: &str, roots: &[&str]) -> RunResult {
    run_ruby_packages(files, entry, roots, &[])
}

/// `run_ruby_project` plus `.gemspec` gem directories,
/// also relative to the temp project dir.
#[allow(dead_code)]
pub fn run_ruby_packages(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
    package_dirs: &[&str],
) -> RunResult {
    let (_dir, source, opts) = write_project(files, entry, roots, package_dirs);
    compile_link_run(&source, &opts, &[], &[])
}

/// `run_ruby_project` with `--embed-sources` roots -- the ruby source that
/// travels INSIDE the program, for a require only the run time can resolve.
#[allow(dead_code)]
pub fn run_ruby_project_embedded(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
    embed: &[&str],
    env: &[(&str, &str)],
) -> RunResult {
    let (dir, source, mut opts) = write_project(files, entry, roots, &[]);
    opts.embed_sources = embed.iter().map(|r| dir.join(r)).collect();
    compile_link_run(&source, &opts, env, &[])
}

/// `compile_project` under `--strict-static-require`.
#[allow(dead_code)]
pub fn compile_project_strict(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
) -> Result<std::path::PathBuf, String> {
    let (dir, entry_source, mut opts) = write_project(files, entry, roots, &[]);
    opts.strict_static_require = true;
    match zeo::check_program_with(&entry_source, &opts) {
        Ok(()) => Ok(dir),
        Err(e) => Err(String::from(e)),
    }
}

/// The compile-only half of `run_ruby_project`, exposed separately so
/// negative-path tests can assert on the compile error without a build.
/// Returns the generated Rust plus the temp project dir (caller cleans up
/// on the success path; the error path cleans up here).
#[allow(dead_code)]
pub fn compile_project(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
) -> Result<std::path::PathBuf, String> {
    compile_packages(files, entry, roots, &[])
}

/// `compile_project` plus package directories -- see `run_ruby_packages`.
#[allow(dead_code)]
pub fn compile_packages(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
    package_dirs: &[&str],
) -> Result<std::path::PathBuf, String> {
    let (dir, entry_source, opts) = write_project(files, entry, roots, package_dirs);
    match zeo::check_program_with(&entry_source, &opts) {
        Ok(()) => Ok(dir),
        // The message alone -- negative-path tests assert on the same text
        // the pre-typed-error harness always saw.
        Err(e) => Err(String::from(e)),
    }
}

/// Write `files` into a per-project temp directory and build the compile
/// options that name `entry`. Returns `(dir, entry source, opts)`.
///
/// The directory is named from the project's own CONTENT, not the pid:
/// `__FILE__`/`__dir__` bake this absolute path into the program, so a
/// pid-named directory changed every run.
///
/// It is never REMOVED, for the same reason. The hash makes the directory
/// self-consistent -- the same hash always means the same files -- so there
/// is nothing stale to clear, while a cleanup would race any concurrent
/// test whose project has identical content and is compiling from it right
/// now. The tree lives in the system temp directory and costs kilobytes.
fn write_project(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
    package_dirs: &[&str],
) -> (std::path::PathBuf, String, zeo::CompileOptions) {
    let mut hasher = std::hash::DefaultHasher::new();
    for (rel, source) in files {
        std::hash::Hash::hash(&(rel, source), &mut hasher);
    }
    std::hash::Hash::hash(&(entry, roots, package_dirs), &mut hasher);
    let dir = std::env::temp_dir().join(format!(
        "zeo-test-proj-{:016x}",
        std::hash::Hasher::finish(&hasher)
    ));
    for (rel, source) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("creating test project dirs");
        }
        std::fs::write(&path, source).expect("writing test project file");
    }
    let entry_path = dir.join(entry);
    let entry_source = std::fs::read_to_string(&entry_path).expect("entry must be in `files`");
    let opts = zeo::CompileOptions {
        input_path: Some(entry_path),
        load_roots: roots.iter().map(|r| dir.join(r)).collect(),
        package_dirs: package_dirs.iter().map(|r| dir.join(r)).collect(),
        // The disclosure report/warnings default OFF for the in-process test
        // harness (the `--no-report` case) -- it already knows substitutions
        // happen and must not litter the tree or dirty asserted stderr.
        ..Default::default()
    };
    (dir, entry_source, opts)
}

/// `run_ruby` plus environment variables and argv for the COMPILED BINARY's
/// own invocation -- backs the scheduler-config tests
/// (`ZEO_THREADS=N` / `--no-gvl` must change nothing observable for a
/// well-behaved program).
#[allow(dead_code)] // each test binary compiles its own copy of this module
pub fn run_ruby_configured(source: &str, env: &[(&str, &str)], args: &[&str]) -> RunResult {
    compile_link_run(source, &Default::default(), env, args)
}
