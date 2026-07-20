//! In-process test harness: compiles Ruby source via `spinelc::compile_to_rust`
//! directly (no subprocess spawn for the compiler itself), then shells out
//! only for the unavoidable parts -- `cargo build`-ing the generated program
//! and running the resulting binary -- capturing stdout/stderr/exit status
//! separately so tests can assert on each independently. This is the new
//! default tier for spinelc test coverage, replacing most `.expected`-file
//! testing going forward; `examples/*.rb` + `.expected` (driven by `xtask`)
//! remains as a smaller "real CLI + real `ruby`-oracle" smoke suite.

pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub status: std::process::ExitStatus,
}

pub fn run_ruby(source: &str) -> RunResult {
    run_ruby_configured(source, &[], &[])
}

/// Multi-file harness for Phase 14.1's compile-time `require` resolution:
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
    let result = compile_packages(files, entry, roots, package_dirs);
    let (compiled, dir) = match result {
        Ok(v) => v,
        Err(e) => panic!("compile_to_rust_with failed: {e}"),
    };
    let rust_source = &compiled.rust_source;

    let bin = std::env::temp_dir().join(format!(
        "spinelc-test-bin-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let runtime = spinelc::build::Runtime::for_eval(compiled.needs_eval_vm);
    spinelc::build::ensure_runtime_built(spinelc::build::Profile::Debug, runtime)
        .expect("building spinel-rt for the e2e harness");
    spinelc::build::build_binary(
        rust_source,
        &bin,
        spinelc::build::Linkage::Dynamic,
        spinelc::build::Profile::Debug,
        runtime,
    )
        .unwrap_or_else(|e| {
            panic!("build_binary failed: {e}\n--- generated Rust ---\n{rust_source}")
        });
    let out = std::process::Command::new(&bin)
        .output()
        .unwrap_or_else(|e| panic!("running compiled binary: {e}"));
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_dir_all(&dir);

    RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        status: out.status,
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
) -> Result<(spinelc::CompileOutput, std::path::PathBuf), String> {
    compile_packages(files, entry, roots, &[])
}

/// `compile_project` plus package directories -- see `run_ruby_packages`.
#[allow(dead_code)]
pub fn compile_packages(
    files: &[(&str, &str)],
    entry: &str,
    roots: &[&str],
    package_dirs: &[&str],
) -> Result<(spinelc::CompileOutput, std::path::PathBuf), String> {
    let dir = std::env::temp_dir().join(format!(
        "spinelc-test-proj-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, source) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("creating test project dirs");
        }
        std::fs::write(&path, source).expect("writing test project file");
    }
    let entry_path = dir.join(entry);
    let entry_source = std::fs::read_to_string(&entry_path).expect("entry must be in `files`");
    let opts = spinelc::CompileOptions {
        input_path: Some(entry_path),
        load_roots: roots.iter().map(|r| dir.join(r)).collect(),
        package_dirs: package_dirs.iter().map(|r| dir.join(r)).collect(),
        // The disclosure report/warnings default OFF for the in-process test
        // harness (the `--no-report` case) -- it already knows substitutions
        // happen and must not litter the tree or dirty asserted stderr.
        ..Default::default()
    };
    match spinelc::compile_to_rust_with(&entry_source, &opts) {
        Ok(rust) => Ok((rust, dir)),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// `run_ruby` plus environment variables and argv for the COMPILED BINARY's
/// own invocation -- backs the Phase 13.4 scheduler-config tests
/// (`SPINEL_THREADS=N` / `--no-gvl` must change nothing observable for a
/// well-behaved program).
#[allow(dead_code)] // each test binary compiles its own copy of this module
pub fn run_ruby_configured(source: &str, env: &[(&str, &str)], args: &[&str]) -> RunResult {
    // `compile_to_rust_with` (not the flag-less `compile_to_rust`) so the eval
    // VM detection rides along -- a program that reaches runtime `eval` must
    // link the `Eval` runtime variant, or its eval would hit the lean stub.
    let compiled = spinelc::compile_to_rust_with(source, &Default::default())
        .unwrap_or_else(|e| panic!("compile_to_rust_with failed: {e}"));
    let rust_source = &compiled.rust_source;

    let bin = std::env::temp_dir().join(format!(
        "spinelc-test-bin-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let runtime = spinelc::build::Runtime::for_eval(compiled.needs_eval_vm);
    spinelc::build::ensure_runtime_built(spinelc::build::Profile::Debug, runtime)
        .expect("building spinel-rt for the e2e harness");
    spinelc::build::build_binary(rust_source, &bin, spinelc::build::Linkage::Dynamic, spinelc::build::Profile::Debug, runtime).unwrap_or_else(|e| {
        panic!("build_binary failed: {e}\n--- generated Rust ---\n{rust_source}")
    });

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
