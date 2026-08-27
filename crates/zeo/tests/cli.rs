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

    // Option-looking ARGV needs no separator: option parsing stops at the
    // file name, ruby's rule, which is what lets `zeo test.rb --seed 42`
    // drive a test framework. A literal `--` after the file is ARGV too --
    // also ruby's answer.
    let out = zeo().arg(&rb).args(["-n", "x"]).output().expect("spawn zeo");
    assert_eq!(stdout_of(&out), "[\"-n\", \"x\"]\n");
    assert_eq!(out.status.code(), Some(7));

    let out = zeo()
        .arg(&rb)
        .args(["--", "-n"])
        .output()
        .expect("spawn zeo");
    assert_eq!(stdout_of(&out), "[\"--\", \"-n\"]\n");
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

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn dash_r_requires_a_library_before_the_first_line() {
    let dir = scratch("dash-r");
    write(&dir, "lib/greet.rb", "GREETING = \"hi\"\n");
    let rb = write(&dir, "t.rb", "puts GREETING\n");

    // The spaced spelling, and the attached one ruby also takes.
    for spelling in [vec!["-r", "greet"], vec!["-rgreet"]] {
        let out = zeo()
            .arg("-I")
            .arg(dir.join("lib"))
            .args(&spelling)
            .arg(&rb)
            .output()
            .expect("spawn zeo");
        assert_eq!(
            stdout_of(&out),
            "hi\n",
            "{spelling:?} -- stderr: {}",
            stderr_of(&out)
        );
    }
}

#[test]
fn several_dash_r_run_in_the_order_given() {
    let dir = scratch("dash-r-order");
    write(&dir, "lib/one.rb", "puts \"one\"\n");
    write(&dir, "lib/two.rb", "puts \"two\"\n");
    let rb = write(&dir, "t.rb", "puts \"main\"\n");

    let out = zeo()
        .arg("-I")
        .arg(dir.join("lib"))
        .args(["-r", "two", "-r", "one"])
        .arg(&rb)
        .output()
        .expect("spawn zeo");
    assert_eq!(
        stdout_of(&out),
        "two\none\nmain\n",
        "stderr: {}",
        stderr_of(&out)
    );
}

#[test]
fn dash_r_of_a_missing_library_is_reported() {
    let dir = scratch("dash-r-missing");
    let rb = write(&dir, "t.rb", "puts 1\n");

    let out = zeo()
        .args(["-r", "no_such_library_anywhere"])
        .arg(&rb)
        .output()
        .expect("spawn zeo");
    assert_ne!(out.status.code(), Some(0), "a missing -r must not succeed");
    let said = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(
        said.contains("no_such_library_anywhere"),
        "the failure must name the library: {said}"
    );
}

#[test]
fn an_unknown_feature_lists_the_ones_that_exist() {
    let out = zeo()
        .args(["--disable=teleport", "-e", "puts 1"])
        .output()
        .expect("spawn zeo");
    let err = stderr_of(&out);
    assert!(err.contains("gems"), "{err}");
    assert!(err.contains("rubyopt"), "{err}");
}

#[test]
fn enabling_a_feature_zeo_has_nothing_behind_reports_why() {
    let out = zeo()
        .args(["--enable=syntax_suggest", "-e", "puts 1"])
        .output()
        .expect("spawn zeo");
    assert_ne!(out.status.code(), Some(0));
    assert!(stderr_of(&out).contains("syntax_suggest"), "{}", stderr_of(&out));

    // Turning it off names the state already in force, so it runs.
    let out = zeo()
        .args(["--disable=syntax_suggest", "-e", "puts 1"])
        .output()
        .expect("spawn zeo");
    assert_eq!(stdout_of(&out), "1\n", "stderr: {}", stderr_of(&out));
}

#[test]
fn both_of_rubys_spellings_reach_one_dial() {
    for flag in ["--disable=gems", "--disable-gems", "--disable=all"] {
        let out = zeo().args([flag, "-e", "puts 1"]).output().expect("spawn zeo");
        assert_eq!(stdout_of(&out), "1\n", "{flag} -- stderr: {}", stderr_of(&out));
    }
}

#[test]
fn disabling_rubyopt_stops_it_being_read() {
    // An illegal switch in RUBYOPT is an error -- unless the dial that reads
    // RUBYOPT at all has been turned off.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zeo"));
    cmd.env_remove("RUBYLIB").env("RUBYOPT", "-Q");
    let out = cmd.args(["-e", "puts 1"]).output().expect("spawn zeo");
    assert_ne!(out.status.code(), Some(0), "RUBYOPT is read by default");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zeo"));
    cmd.env_remove("RUBYLIB").env("RUBYOPT", "-Q");
    let out = cmd
        .args(["--disable=rubyopt", "-e", "puts 1"])
        .output()
        .expect("spawn zeo");
    assert_eq!(stdout_of(&out), "1\n", "stderr: {}", stderr_of(&out));
}
