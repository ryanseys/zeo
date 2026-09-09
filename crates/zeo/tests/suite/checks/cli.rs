//! End-to-end tests of the `zeo` BINARY -- the real CLI, spawned as a
//! process, not the library API the golden harness drives. What lives here
//! is exactly the surface `main.rs` owns: mode selection (run vs artifact),
//! ARGV forwarding, exit-status forwarding, `-I` handling.
//!
//! Each test writes its sources into its own temp dir; programs are tiny,
//! so each compile is cheap. Nothing here requires a whole gem: the exit
//! status an `at_exit` handler sets is pinned by the
//! `at_exit_status_override` golden.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn zeo() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zeo"));
    // Ambient ruby config must not leak into the parse (RUBYOPT would be
    // consulted; a user's RUBYLIB would widen the load path).
    cmd.env_remove("RUBYOPT").env_remove("RUBYLIB");
    // These tests take the DEFAULT run path, which is the compiled-program
    // cache -- exactly the surface they exist to check. They get their own
    // cache directory so they neither read nor grow the developer's. It is
    // shared across the tests in this binary and NOT wiped per call, because
    // a second run finding the first run's entry is a thing under test.
    cmd.env("ZEO_PROGRAM_CACHE", program_cache());
    cmd
}

/// One cache directory for this test binary, created once.
fn program_cache() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-cli-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the program cache dir");
    dir
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
    let out = zeo()
        .arg(&rb)
        .args(["-n", "x"])
        .output()
        .expect("spawn zeo");
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
fn a_build_flag_after_the_file_is_argv_and_builds_nothing() {
    // Ruby's rule costs something, and this is the bill. `zeo prog.rb -o bin`
    // reads like a compile and is a RUN with `["-o", "bin"]` for ARGV: no
    // binary, and exit 0 to say so.
    //
    // This is not hypothetical. The criterion bank spelled its compile that
    // way, so it died on its first benchmark spawning a file nothing wrote,
    // and `zeo --help` documented the same spelling in two of its mode lines.
    let dir = scratch("flag-after-file");
    let rb = write(&dir, "hello.rb", "p ARGV\n");

    for flag in [vec!["-o", "out"], vec!["--compile"]] {
        let out = zeo().arg(&rb).args(&flag).output().expect("spawn zeo");
        assert!(
            out.status.success(),
            "{flag:?} after the file should RUN, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let argv: Vec<String> = flag.iter().map(|s| format!("{s:?}")).collect();
        assert_eq!(
            stdout_of(&out),
            format!("[{}]\n", argv.join(", ")),
            "{flag:?} after the file should reach the program as ARGV"
        );
    }
    assert!(
        !dir.join("out").exists(),
        "`-o out` after the file built one"
    );
    assert!(
        !dir.join("hello").exists(),
        "`--compile` after the file built one"
    );

    // The help text has to spell it the way that works, or it teaches the bug.
    let help = zeo().arg("--help").output().expect("spawn zeo");
    let text = stdout_of(&help);
    for want in ["-o <path> <input.rb>", "--compile <input.rb>"] {
        assert!(
            text.contains(want),
            "`zeo --help` has no `{want}` line:\n{text}"
        );
    }
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
    assert!(
        stderr_of(&out).contains("syntax_suggest"),
        "{}",
        stderr_of(&out)
    );

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
        let out = zeo()
            .args([flag, "-e", "puts 1"])
            .output()
            .expect("spawn zeo");
        assert_eq!(
            stdout_of(&out),
            "1\n",
            "{flag} -- stderr: {}",
            stderr_of(&out)
        );
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

#[test]
fn a_library_named_in_the_runtime_load_dial_loads_at_run_time() {
    // The isolation dial. The SAME source compiles both ways -- once with the
    // library spliced in, once with the require surviving to `Kernel#require`
    // -- so a difference in what the program prints is a difference in the
    // loader, which is the whole question the dial exists to answer.
    let dir = scratch("runtime-load");
    write(
        &dir,
        "lib/greet.rb",
        "require \"greet/name\"\nmodule Greet\n  def self.hello = \"hi #{NAME}\"\nend\n",
    );
    write(
        &dir,
        "lib/greet/name.rb",
        "module Greet\n  NAME = \"there\"\nend\n",
    );
    let rb = write(
        &dir,
        "t.rb",
        "$LOAD_PATH.unshift File.expand_path(\"lib\", __dir__)\nrequire \"greet\"\nputs Greet.hello\n",
    );

    let compiled_in = zeo().arg(&rb).output().expect("spawn zeo");
    assert_eq!(
        stdout_of(&compiled_in),
        "hi there\n",
        "stderr: {}",
        stderr_of(&compiled_in)
    );

    let at_run_time = zeo()
        .env("ZEO_DEBUG_RUNTIME_LOAD", "greet")
        .arg(&rb)
        .output()
        .expect("spawn zeo");
    assert_eq!(
        stdout_of(&at_run_time),
        "hi there\n",
        "stderr: {}",
        stderr_of(&at_run_time)
    );

    // A name the dial does NOT hold is untouched, so the dial cannot quietly
    // move a library nobody asked about.
    let other = zeo()
        .env("ZEO_DEBUG_RUNTIME_LOAD", "greeting")
        .arg(&rb)
        .output()
        .expect("spawn zeo");
    assert_eq!(
        stdout_of(&other),
        "hi there\n",
        "stderr: {}",
        stderr_of(&other)
    );

    // `irb` names `irb/init` too -- a library defers WHOLE, or the run
    // measures a mixture of the two loaders rather than either one. A BUNDLED
    // gem is what makes the deferral observable: the compiler spliced nothing
    // under the deferred name, so the sub-file can only arrive through the
    // runtime loader's own search of the bundled roots. Loading (ruby's
    // answer -- rubygems resolves `irb/init` from the activated gem) proves
    // the deferral reached run time AND the runtime loader served it.
    let sub_file = zeo()
        .env("ZEO_DEBUG_RUNTIME_LOAD", "irb")
        .args(["-e", "p(require \"irb/init\")"])
        .output()
        .expect("spawn zeo");
    assert_eq!(
        stdout_of(&sub_file),
        "true\n",
        "a sub-file of a deferred library loads at run time -- stderr: {}",
        stderr_of(&sub_file)
    );
}

/// A second run of an unchanged program does not compile it again.
///
/// The cache is what makes `zeo gem --version` 0.4s instead of 5s, and its
/// whole risk is serving a stale answer -- so the two halves are checked
/// together: the entry appears, and an EDIT to a file the program reads is
/// still seen.
#[test]
fn a_second_run_comes_from_the_program_cache() {
    let dir = scratch("cache-hit");
    write(&dir, "lib/answer.rb", "def answer = 41\n");
    let rb = write(&dir, "t.rb", "require_relative 'lib/answer'\nputs answer\n");

    let first = zeo().arg(&rb).output().expect("zeo runs");
    assert_eq!(stdout_of(&first).trim(), "41");
    let entries = std::fs::read_dir(program_cache())
        .expect("the cache dir is readable")
        .count();
    assert!(entries > 0, "the first run recorded nothing");

    let second = zeo().arg(&rb).output().expect("zeo runs");
    assert_eq!(stdout_of(&second).trim(), "41", "the cached run disagreed");

    // A required file changed. Serving the cached binary here would run the
    // previous version of the user's program, which is the one failure this
    // whole mechanism must not have.
    write(&dir, "lib/answer.rb", "def answer = 42\n");
    let third = zeo().arg(&rb).output().expect("zeo runs");
    assert_eq!(
        stdout_of(&third).trim(),
        "42",
        "an edited require was served from the cache"
    );
}

/// `ZEO_CACHE=0` runs the program without consulting or filling the cache --
/// the escape hatch for anyone who needs to know they are running a fresh
/// compile.
#[test]
fn the_cache_can_be_turned_off() {
    let dir = scratch("cache-off");
    let rb = write(&dir, "t.rb", "puts 7\n");
    let cache = std::env::temp_dir().join(format!("zeo-cli-cache-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    std::fs::create_dir_all(&cache).expect("create the cache dir");

    let out = zeo()
        .env("ZEO_CACHE", "0")
        .env("ZEO_PROGRAM_CACHE", &cache)
        .arg(&rb)
        .output()
        .expect("zeo runs");
    assert_eq!(stdout_of(&out).trim(), "7");
    assert_eq!(
        std::fs::read_dir(&cache).expect("readable").count(),
        0,
        "ZEO_CACHE=0 still wrote to the cache"
    );
    let _ = std::fs::remove_dir_all(&cache);
}

/// A rebuilt RUNTIME ARCHIVE misses the cache. A zeo-rt body fix can leave
/// the compiler binary untouched (the projected class surface is identical,
/// so cargo never relinks `zeo`), and a key on the exe alone would keep
/// serving programs linked against the stale runtime -- the one staleness
/// no source manifest can see. The archive's identity is in the key; this
/// pins it.
#[test]
fn a_rebuilt_runtime_archive_misses_the_cache() {
    let dir = scratch("cache-archive");
    let rb = write(&dir, "t.rb", "puts 9\n");
    let cache = std::env::temp_dir().join(format!("zeo-cli-cache-arch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    std::fs::create_dir_all(&cache).expect("create the cache dir");

    let run = |cache: &Path| {
        let out = zeo()
            .env("ZEO_PROGRAM_CACHE", cache)
            .arg(&rb)
            .output()
            .expect("zeo runs");
        assert_eq!(stdout_of(&out).trim(), "9");
    };
    let entries = |cache: &Path| std::fs::read_dir(cache).expect("readable").count();
    run(&cache);
    assert_eq!(entries(&cache), 1);
    run(&cache);
    assert_eq!(entries(&cache), 1, "an unchanged build must hit");

    // A fresh modification time is what a runtime rebuild leaves behind.
    // Bumped FORWARD (so the dev-tree staleness rule never triggers a real
    // cargo build) and restored after, to keep the window where concurrent
    // tests compute a different key as small as possible.
    let archive = Path::new(env!("CARGO_BIN_EXE_zeo"))
        .parent()
        .expect("the zeo binary has a parent")
        .join("libzeo.a");
    let original = std::fs::metadata(&archive)
        .and_then(|m| m.modified())
        .expect("the archive has a modification time");
    let f = std::fs::OpenOptions::new()
        .append(true)
        .open(&archive)
        .expect("open the archive");
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
        .expect("bump the archive's mtime");
    run(&cache);
    let after = entries(&cache);
    let _ = f.set_modified(original);
    assert_eq!(after, 2, "a rebuilt runtime archive must miss");
    let _ = std::fs::remove_dir_all(&cache);
}

/// An explicit `--backend` means the caller chose how to run, so the cache
/// stays out of it. The golden harness relies on this: it spawns every one
/// of its thousands of children with `--backend jit`.
#[test]
fn an_explicit_backend_skips_the_cache() {
    let dir = scratch("cache-explicit");
    let rb = write(&dir, "t.rb", "puts 8\n");
    let cache = std::env::temp_dir().join(format!("zeo-cli-cache-jit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    std::fs::create_dir_all(&cache).expect("create the cache dir");

    let out = zeo()
        .env("ZEO_PROGRAM_CACHE", &cache)
        .args(["--backend", "jit"])
        .arg(&rb)
        .output()
        .expect("zeo runs");
    assert_eq!(stdout_of(&out).trim(), "8");
    assert_eq!(
        std::fs::read_dir(&cache).expect("readable").count(),
        0,
        "an explicit --backend still wrote to the cache"
    );
    let _ = std::fs::remove_dir_all(&cache);
}

/// A cached run reports the PROGRAM's name, not the cache entry's path.
///
/// `$0` is argv[0], and the cache execs a binary that lives under the cache
/// directory. Without an explicit `arg0` every program saw a path it had
/// never heard of -- and mkmf, which derives an extension's `srcdir` from
/// `$0`, wrote every generated Makefile pointing into the cache.
#[test]
fn a_cached_run_keeps_the_programs_own_name() {
    let dir = scratch("cache-arg0");
    let rb = write(
        &dir,
        "who.rb",
        "puts $0\nputs $PROGRAM_NAME\nputs __FILE__\n",
    );

    let want = format!("{0}\n{0}\n{0}\n", rb.display());
    for run in ["first", "cached"] {
        let out = zeo().arg(&rb).output().expect("zeo runs");
        assert_eq!(
            stdout_of(&out),
            want,
            "the {run} run named the wrong program, stderr: {}",
            stderr_of(&out)
        );
    }
}

/// `--link` is in the help, and so is its env spelling: a tool that wants to
/// know whether this zeo can carry a section probes `zeo --help` for it.
#[test]
fn the_help_names_link_and_its_env_spelling() {
    let help = zeo().arg("--help").output().expect("spawn zeo");
    let text = stdout_of(&help);
    for want in ["--link <arg>", "ZEO_LINK_ARGS"] {
        assert!(text.contains(want), "`zeo --help` has no `{want}`:\n{text}");
    }
}

/// Link arguments describe a linked binary. A run the in-process JIT takes
/// links nothing, so it says so -- once -- and still runs the program.
#[test]
fn link_args_on_an_in_process_run_warn_and_run() {
    let out = zeo()
        .env("ZEO_CACHE", "0")
        .args(["--link", "-Wl,-x", "-e", "puts 1"])
        .output()
        .expect("spawn zeo");
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "1\n");
    let warnings = stderr_of(&out)
        .lines()
        .filter(|l| l.contains("--link arguments apply to a linked binary only"))
        .count();
    assert_eq!(warnings, 1, "stderr: {}", stderr_of(&out));
}
