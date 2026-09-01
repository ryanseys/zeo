//! The pure-Ruby gem tier under CRUBY. Each zeo-authored port in
//! `crates/zeo-rt/gems/<name>/` runs the SAME committed golden under the
//! pinned ruby with its `lib/` on `-I`, diffed against the golden's
//! `.expected` -- so the port, zeo's native ext (the golden suite's own run)
//! and ruby's C gem (the `.expected`'s source) are all held to one text.
//! This is also the port's fast dev loop: no zeo compile anywhere in it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    crate::paths::workspace_root()
}

/// The pinned ruby, or `None` when this machine has none to run the port.
fn oracle_ruby() -> Option<PathBuf> {
    let ruby = crate::paths::resolve_ruby(&root());
    Command::new(&ruby)
        .arg("-v")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| ruby)
}

/// `ruby -I crates/zeo-rt/gems/<gem>/lib <args>`, ambient require hooks
/// stripped so nothing but the pure tree and ruby's own stdlib can answer.
fn pure_ruby(ruby: &Path, gem: &str, args: &[&dyn AsRef<std::ffi::OsStr>]) -> std::process::Output {
    let mut cmd = Command::new(ruby);
    cmd.arg("-I")
        .arg(root().join("crates/zeo-rt/gems").join(gem).join("lib"))
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB");
    for a in args {
        cmd.arg(a.as_ref());
    }
    cmd.output().expect("ruby spawns")
}

/// One golden, three implementations, one `.expected`: the committed text
/// came from the C gem, the golden suite runs zeo's native ext against it,
/// and this run holds the pure port to it too.
fn golden_matches(gem: &str, golden: &str) {
    let Some(ruby) = oracle_ruby() else {
        eprintln!("skipping: this machine has no ruby to run the pure port");
        return;
    };
    let program = root().join("tests").join(format!("{golden}.rb"));
    let expected = std::fs::read_to_string(root().join("tests").join(format!("{golden}.rb.expected")))
        .expect("the golden's .expected is committed");
    let out = pure_ruby(&ruby, gem, &[&program]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the pure {gem} run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout,
        expected,
        "pure {gem} under ruby diverged from the C gem's recorded output"
    );
}

#[test]
fn the_pure_string_scanner_matches_the_c_gems_recorded_output() {
    golden_matches(
        "strscan",
        "a_string_scanner_answers_the_same_off_either_implementation",
    );
}

#[test]
fn the_pure_string_io_matches_the_c_gems_recorded_output() {
    golden_matches(
        "stringio",
        "a_string_io_answers_the_same_off_either_implementation",
    );
}

/// The vendored UPSTREAM suite (ruby/stringio's own test_stringio.rb at the
/// locked v3.2.0 tag) against the pure port, under ruby. The driver refuses
/// to run if the pure tree did not win the require.
#[test]
fn the_pure_string_io_passes_the_upstream_suite() {
    let Some(ruby) = oracle_ruby() else {
        eprintln!("skipping: this machine has no ruby to run the pure port");
        return;
    };
    let out = Command::new(&ruby)
        .arg(root().join("crates/zeo-rt/gems/stringio/test/run_pure.rb"))
        .env("BUNDLE_GEMFILE", root().join("Gemfile"))
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("ruby spawns");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains(", 0 failures, 0 errors,"),
        "upstream stringio suite failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The vendored UPSTREAM suite (ruby/strscan's own test_stringscanner.rb at
/// the locked v3.1.8 tag) against the pure port, under ruby. The driver
/// refuses to run if the pure tree did not win the require.
#[test]
fn the_pure_string_scanner_passes_the_upstream_suite() {
    let Some(ruby) = oracle_ruby() else {
        eprintln!("skipping: this machine has no ruby to run the pure port");
        return;
    };
    let out = Command::new(&ruby)
        .arg(root().join("crates/zeo-rt/gems/strscan/test/run_pure.rb"))
        .env("BUNDLE_GEMFILE", root().join("Gemfile"))
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("ruby spawns");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains(", 0 failures, 0 errors,"),
        "upstream suite failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The vendored upstream monitor suite (ruby/ruby's test_monitor.rb at the
/// ruby-headers.lock rev) against zeo's RUBY HALF, overlaid on CRuby's own
/// C Monitor -- which exports the same enter/exit/wait_for_cond surface as
/// zeo's Rust half, so the delegation layer is what differs.
#[test]
fn the_monitor_ruby_half_passes_the_upstream_suite() {
    let Some(ruby) = oracle_ruby() else {
        eprintln!("skipping: this machine has no ruby to run the suite");
        return;
    };
    let out = Command::new(&ruby)
        .arg(root().join("crates/zeo-rt/ext/monitor/test/run_pure.rb"))
        .env("BUNDLE_GEMFILE", root().join("Gemfile"))
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("ruby spawns");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains(", 0 failures, 0 errors,"),
        "upstream monitor suite failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The zeo-native stdlib NEVER arms the GVL: a program requiring the whole
/// surface runs with the parallel default and the sole-thread claim intact.
/// The sentinel is the runtime's own arming log line (a shared constant, so
/// producer and instrument cannot drift), watched at debug level. Only a
/// genuine third-party C extension may print it.
#[test]
fn the_zeo_native_stdlib_never_arms_the_gvl() {
    let result = crate::support::run_ruby_configured(
        r#"
        require "json"
        require "yaml"
        require "date"
        require "zlib"
        require "digest"
        require "openssl"
        require "socket"
        require "stringio"
        require "strscan"
        require "base64"
        require "securerandom"
        require "monitor"
        require "etc"
        require "fcntl"
        require "bigdecimal"
        require "nkf"
        require "syslog"
        require "io/console"
        require "erb"
        require "csv"
        require "time"
        require "tempfile"
        require "fileutils"
        require "logger"
        require "uri"
        puts "loaded"
        "#,
        &[("ZEO_LOG", "debug")],
        &[],
    );
    assert!(
        result.status.success(),
        "the stdlib program failed: {}",
        result.stderr
    );
    assert!(result.stdout.contains("loaded"), "stdout: {}", result.stdout);
    assert!(
        !result.stderr.contains(zeo_rt::gvl::CEXT_ARMED_SENTINEL),
        "requiring the zeo-native stdlib armed the GVL:\n{}",
        result.stderr
    );
}

/// The `-I` really shadows ruby's own strscan: the loaded feature is the
/// pure tree's `.rb`, and no native strscan library loads beside it. Without
/// this, a miss in the pure tree would fall through to the C gem and the
/// diff above would prove nothing.
#[test]
fn the_pure_string_scanner_shadows_the_c_gem() {
    let Some(ruby) = oracle_ruby() else {
        eprintln!("skipping: this machine has no ruby to run the pure port");
        return;
    };
    let probe = r#"require "strscan"
        loaded = $LOADED_FEATURES.grep(/strscan/)
        puts loaded.length
        puts loaded.first
    "#;
    let out = pure_ruby(&ruby, "strscan", &[&"-e", &probe]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("1"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let feature = lines.next().unwrap_or_default();
    assert!(
        feature.ends_with("crates/zeo-rt/gems/strscan/lib/strscan.rb"),
        "require resolved {feature}, not the pure tree"
    );
}
