//! End-to-end tests of the `zeo` BINARY -- the real CLI, spawned as a
//! process, not the library API the golden harness drives. What lives here
//! is exactly the surface `main.rs` owns: mode selection (run vs artifact),
//! ARGV forwarding, exit-status forwarding, `-I` handling.
//!
//! Each test writes its sources into its own temp dir; programs are tiny,
//! so each compile is cheap. The one deliberately heavy case
//! (`a_failing_minitest_run_exits_nonzero`) splices the whole bundled
//! minitest graph -- it is the wave's load-bearing promise
//! (`zeo test.rb` fails when the tests fail) and worth its compile.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn zeo() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zeo"));
    // Ambient ruby config must not leak into the parse (RUBYOPT would be
    // consulted; a user's RUBYLIB would widen the load path).
    cmd.env_remove("RUBYOPT").env_remove("RUBYLIB");
    cmd
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
    std::fs::write(&path, content).expect("write source");
    path
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_bare_file_runs_with_argv_and_exit_status() {
    let dir = scratch("bare-run");
    let rb = write(&dir, "t.rb", "p ARGV\nexit 7\n");

    let out = zeo()
        .arg(&rb)
        .args(["a", "-x"])
        .output()
        .expect("spawn zeo");
    assert_eq!(
        stdout_of(&out),
        "[\"a\", \"-x\"]\n",
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(7));

    // Option-looking ARGV goes after `--` (before any positional, `-n` would
    // otherwise be parsed as a zeo option and rejected).
    let out = zeo()
        .arg(&rb)
        .args(["--", "-n", "x"])
        .output()
        .expect("spawn zeo");
    assert_eq!(stdout_of(&out), "[\"-n\", \"x\"]\n");
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn dash_i_adds_a_require_root_like_ruby() {
    // The `zeo -Itest test/foo_test.rb` shape: the helper resolves through
    // the -I root, and the attached `-I=` spelling works end to end.
    let dir = scratch("dash-i");
    write(&dir, "test/helper.rb", "HELPED = 41\n");
    let rb = write(&dir, "test/t.rb", "require \"helper\"\nputs HELPED + 1\n");

    let out = zeo()
        .current_dir(&dir)
        .arg("-I=test")
        .arg(&rb)
        .output()
        .expect("spawn zeo");
    assert_eq!(
        stdout_of(&out),
        "42\n",
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn compile_writes_the_default_binary_and_runs_nothing() {
    let dir = scratch("compile-flag");
    let rb = write(&dir, "hello.rb", "puts \"ran\"\n");

    let out = zeo().arg("--compile").arg(&rb).output().expect("spawn zeo");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Compiling must not execute the program...
    assert_eq!(stdout_of(&out), "");
    // ...and the artifact lands at the input path minus its extension.
    let bin = dir.join("hello");
    let ran = Command::new(&bin).output().expect("run the artifact");
    assert_eq!(stdout_of(&ran), "ran\n");
}

#[test]
fn a_failing_minitest_run_exits_nonzero() {
    // THE promise of run-by-default: `zeo test.rb` reports test failure
    // through the exit status, the way `ruby test.rb` does. Minitest sets
    // it via `exit` inside its autorun at_exit handler.
    let dir = scratch("minitest-fail");
    let rb = write(
        &dir,
        "failing_test.rb",
        "require \"minitest/autorun\"\n\
         class FailingTest < Minitest::Test\n\
           def test_truth\n\
             assert false, \"deliberate\"\n\
           end\n\
         end\n",
    );

    let out = zeo()
        .arg(&rb)
        .args(["--", "--seed", "42"])
        .output()
        .expect("spawn zeo");
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("1 runs, 1 assertions, 1 failures"),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1), "a failing suite must exit 1");
}
