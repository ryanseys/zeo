//! In-process e2e harness: compiles Ruby source through Cranelift to an
//! object file directly -- no subprocess spawn for the compiler itself --
//! links a throwaway binary and runs it, capturing stdout/stderr/exit
//! status separately so tests can assert on each independently. This is the
//! default tier for zeo test coverage; `tests/*.rb` + `.expected` remains
//! as the "real CLI + real `ruby`-oracle" golden suite.
//!
//! The compile-only negative-path helpers (`compile_project` and friends)
//! ask `check_program_with`: they assert front-end and loader POLICY (a
//! `require` that must not resolve, a form that must be rejected) without
//! building anything.

pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub status: std::process::ExitStatus,
}

/// `ZEO_E2E_BACKEND`: which tier runs the e2e programs. Unset or
/// `jit-child` spawns the built `zeo` CLI on the JIT (a ~20ms spawn);
/// `aot` restores the link-a-binary path -- the one tier that shells `cc`
/// per test, which is what made the suite cost minutes. CI's AOT leg runs
/// the suite under `aot` so both tiers stay covered.
fn e2e_backend() -> &'static str {
    static B: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    B.get_or_init(|| match std::env::var("ZEO_E2E_BACKEND") {
        Ok(v) if !v.is_empty() => v,
        _ => "jit-child".to_string(),
    })
}

/// Compile `source` with `opts` and run it with `env`/`args`, handing back
/// what it printed. The one place the e2e tier builds a program; the
/// backend is [`e2e_backend`]'s.
pub fn compile_link_run(
    source: &str,
    opts: &zeo::CompileOptions,
    env: &[(&str, &str)],
    args: &[&str],
) -> RunResult {
    match e2e_backend() {
        "aot" => compile_link_run_aot(source, opts, env, args),
        "jit-child" => run_jit_child(source, opts, env, args),
        other => panic!("unknown ZEO_E2E_BACKEND '{other}' (jit-child or aot)"),
    }
}

/// The AOT tier: object-compile in process, link a throwaway binary, run
/// it. What ships, and what the artifact-shape tests (`linkage.rs`,
/// `debuginfo.rs`) reason about -- they build their own artifacts and never
/// come through here, but the whole suite re-runs on this tier under CI's
/// AOT leg.
pub fn compile_link_run_aot(
    source: &str,
    opts: &zeo::CompileOptions,
    env: &[(&str, &str)],
    args: &[&str],
) -> RunResult {
    // The link needs the archive, which a test run does not build.
    crate::paths::runtime_archive().unwrap_or_else(|e| panic!("{e}"));
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

/// The JIT tier: spawn the built `zeo` CLI on the in-process JIT. The
/// program's exit status -- and a death by signal -- IS the child's, so the
/// `process_exit.rs` assertions keep their meaning, and an `-e` compile is
/// named `-e` by the CLI exactly as the in-process harness named it.
///
/// A program zeo REJECTS exits nonzero with the compiler's own error on
/// stderr, which the caller's assertion then shows. (An in-process
/// `check_program_with` probe once classified rejections up front; it
/// re-ran the whole front end for every test and bought only a nicer panic
/// message, so it went.)
fn run_jit_child(
    source: &str,
    opts: &zeo::CompileOptions,
    env: &[(&str, &str)],
    args: &[&str],
) -> RunResult {
    // No silent drops: every option a test sets must reach the child as a
    // flag, and these two have no consumer among compile_link_run's callers
    // today -- a new caller must extend the forwarding, not lose the option.
    assert!(
        opts.gem_report.is_none() && opts.root_gem.is_none(),
        "the jit-child tier does not forward gem_report/root_gem; extend run_jit_child"
    );
    let cli = crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"));
    let mut cmd = std::process::Command::new(cli);
    cmd.arg("--backend").arg("jit");
    for root in &opts.load_roots {
        cmd.arg("-I").arg(root);
    }
    for dir in &opts.package_dirs {
        cmd.arg("--gems").arg(dir);
    }
    for root in &opts.embed_sources {
        cmd.arg("--embed-sources").arg(root);
    }
    if opts.strict_static_require {
        cmd.arg("--strict-static-require");
    }
    // The external gem store: the CLI requires the pair together, and a
    // path that already IS a lockfile is taken as given by derive_lockfile.
    for store in &opts.gem_paths {
        cmd.arg("--gem-path").arg(store);
    }
    if let Some(lock) = &opts.lockfile {
        cmd.arg("--bundle-gemfile").arg(lock);
    }
    match &opts.input_path {
        Some(path) => cmd.arg(path),
        None => cmd.arg("-e").arg(source),
    };
    // `--` first: the program's args are the PROGRAM's, not CLI options.
    if !args.is_empty() {
        cmd.arg("--").args(args);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("spawning the zeo CLI: {e}"));
    RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        status: out.status,
    }
}

pub fn run_ruby(source: &str) -> RunResult {
    run_ruby_configured(source, &[], &[])
}

/// Multi-file harness for compile-time `require` resolution: writes `files`
/// (relative path -> source) into a fresh project directory under the scratch
/// root, compiles `entry` with that directory as the requiring-file base,
/// `roots` as `-I` search roots and `package_dirs` as `.gemspec` gem
/// directories, then builds and runs it like `run_ruby`.
pub fn run_ruby_packages(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
    package_dirs: &[&str],
) -> RunResult {
    let (_dir, source, opts) = write_project(files, entry, roots, package_dirs);
    compile_link_run(&source, &opts, &[], &[])
}

/// `compile_project` under `--strict-static-require`.
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
pub fn compile_project(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
) -> Result<std::path::PathBuf, String> {
    compile_packages(files, entry, roots, &[])
}

/// `compile_project` plus package directories -- see `run_ruby_packages`.
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
pub fn run_ruby_configured(source: &str, env: &[(&str, &str)], args: &[&str]) -> RunResult {
    compile_link_run(source, &Default::default(), env, args)
}

// ---- C extensions ----
//
// The tree tracks no C (`checks/no_c.rs`), so every extension a test builds
// is written from text into a scratch directory first.

/// `tool --version` runs. A test that builds an extension skips, saying so,
/// on a machine without a C compiler.
pub fn have(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A fresh extension directory: `extconf.rb` beside `<name>.c`, flat, which
/// is the shape mkmf runs in.
pub fn extension_dir(tag: &str, name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-cext-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    std::fs::write(
        dir.join("extconf.rb"),
        format!("require \"mkmf\"\ncreate_makefile({name:?})\n"),
    )
    .expect("write extconf.rb");
    std::fs::write(dir.join(format!("{name}.c")), source).expect("write the C");
    dir
}

/// The fixture store's buildable extension: one module, one method, one
/// String. Anything more would test the C API rather than the
/// build-and-load path the fixture exists for.
const NATIVELIB_C: &str = r#"#include <ruby.h>

static VALUE
nativelib_greet(VALUE self)
{
    (void)self;
    return rb_utf8_str_new_cstr("hello from C");
}

void
Init_nativelib(void)
{
    VALUE mod = rb_define_module("Nativelib");

    rb_define_singleton_method(mod, "greet", nativelib_greet, 0);
    rb_define_const(mod, "BUILT", Qtrue);
}
"#;

/// The fixture gem store (`tests/fixtures/gem_store/store`), copied whole
/// into a scratch directory with `nativelib`'s C written beside its
/// `extconf.rb`. A copy per test: a store is shared and often read only,
/// and a build that wrote into it would be a bug `ffi.rs` asserts against.
pub fn gem_store(tag: &str) -> std::path::PathBuf {
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gem_store/store");
    let store = std::env::temp_dir().join(format!("zeo-gem-store-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&store);
    copy_tree(&fixture, &store);
    std::fs::write(
        store.join("gems/nativelib-1.0.0/ext/nativelib/nativelib.c"),
        NATIVELIB_C,
    )
    .expect("write nativelib.c");
    store
}

fn copy_tree(src: &std::path::Path, dest: &std::path::Path) {
    std::fs::create_dir_all(dest).unwrap_or_else(|e| panic!("creating {}: {e}", dest.display()));
    for entry in std::fs::read_dir(src).unwrap_or_else(|e| panic!("reading {}: {e}", src.display()))
    {
        let entry = entry.expect("a directory entry");
        let (from, to) = (entry.path(), dest.join(entry.file_name()));
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap_or_else(|e| panic!("copying {}: {e}", from.display()));
        }
    }
}
