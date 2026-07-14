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
    let rust_source = spinelc::compile_to_rust(source)
        .unwrap_or_else(|e| panic!("compile_to_rust failed: {e}"));

    let bin = std::env::temp_dir().join(format!(
        "spinelc-test-bin-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    spinelc::build::build_binary(&rust_source, &bin).unwrap_or_else(|e| {
        panic!("build_binary failed: {e}\n--- generated Rust ---\n{rust_source}")
    });

    let out = std::process::Command::new(&bin)
        .output()
        .unwrap_or_else(|e| panic!("running compiled binary: {e}"));
    let _ = std::fs::remove_file(&bin);

    RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        status: out.status,
    }
}
