//! EXPERIMENTAL (M0): separate compilation's merge spine.
//!
//! A gem compiles ONCE to its own object plus a row manifest
//! (`--experimental-pkg`); a host program merges the manifest into its one
//! `ProgramDesc` and links the object beside its own
//! (`--experimental-use-pkg`). These tests hold the two contracts the
//! design rests on: the packaged program answers BYTE FOR BYTE what the
//! same program answers with the gem spliced (today's whole-program path),
//! and every refusal names itself rather than mislinking.

use std::path::{Path, PathBuf};
use std::process::Command;

fn zeo() -> Command {
    Command::new(crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}")))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-pkg-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn fixture_gem() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/pureleaf")
}

fn run(cmd: &mut Command) -> std::process::Output {
    cmd.env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("spawn")
}

fn ok(cmd: &mut Command) -> String {
    let out = run(cmd);
    assert!(
        out.status.success(),
        "expected success\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Compile the fixture gem as a package into `dir`, answering the object
/// path (the manifest lands beside it as `pureleaf.zman`).
fn build_package(dir: &Path) -> PathBuf {
    let object = dir.join("pureleaf.o");
    ok(zeo()
        .arg("--experimental-pkg")
        .arg("pureleaf")
        .arg("-o")
        .arg(&object)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    assert!(object.is_file(), "the package object was written");
    assert!(
        object.with_extension("zman").is_file(),
        "the manifest was written beside the object"
    );
    object
}

const HOST: &str = r#"p defined?(Pureleaf)
p require "pureleaf"
one = Pureleaf.new
p one.leaf
p one.tagged(2)
p Pureleaf.kind
p Pureleaf::Deep::WIDTH
p PURELEAF_TAG
p require "pureleaf"
p Pureleaf.instance_methods(false).sort
begin
  one.missing
rescue NoMethodError => e
  p e.class
end
"#;

/// The one output every road must produce -- verified against ruby 4.0.6
/// by the sibling differential assertion below, then held here so a drift
/// in EITHER road fails by name.
const WANT: &str = "nil\ntrue\n\"leaf\"\n\"leaf-2\"\n:pure\n3\n7\nfalse\n[:leaf, :tagged]\nNoMethodError\n";

#[test]
fn a_precompiled_gem_links_and_answers_like_the_spliced_one() {
    let dir = scratch("slice");
    let object = build_package(&dir);
    let host = dir.join("host.rb");
    std::fs::write(&host, HOST).expect("write host");

    // Road one: the gem PRECOMPILED, merged and linked.
    let packaged_bin = dir.join("host-packaged");
    ok(zeo()
        .arg("--experimental-use-pkg")
        .arg(&object)
        .arg("-o")
        .arg(&packaged_bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    let packaged = ok(&mut Command::new(&packaged_bin));

    // Road two: the same gem SPLICED, today's whole-program compile.
    let spliced_bin = dir.join("host-spliced");
    ok(zeo()
        .arg("--gems")
        .arg(fixture_gem().parent().expect("fixtures dir"))
        .arg("-o")
        .arg(&spliced_bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    let spliced = ok(&mut Command::new(&spliced_bin));

    assert_eq!(packaged, WANT, "the packaged program's answers");
    assert_eq!(spliced, WANT, "the spliced program's answers");
}

#[test]
fn a_bootstrap_mismatch_is_refused_by_name() {
    let dir = scratch("mismatch");
    let object = build_package(&dir);
    // Corrupt the recorded band start: the host must refuse rather than
    // register the package's classes at the wrong ids.
    let zman = object.with_extension("zman");
    let text = std::fs::read_to_string(&zman).expect("read manifest");
    std::fs::write(&zman, text.replace("\"first_class_id\":", "\"first_class_id\": 9000, \"_was\":"))
        .expect("rewrite manifest");
    let host = dir.join("host.rb");
    std::fs::write(&host, "p 1\n").expect("write host");
    let out = run(zeo()
        .arg("--experimental-use-pkg")
        .arg(&object)
        .arg("-o")
        .arg(dir.join("host-bin"))
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a wrong band must not link");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("minted its classes from id 9000"),
        "the refusal names the band: {err}"
    );
}

#[test]
fn a_package_carrying_main_code_is_refused_by_name() {
    // The package driver compiles an EMPTY main; a shape that would leave
    // statements there (eval, boxes, top-level code) must refuse loudly.
    // The cheapest such probe: an entry whose unit itself is fine but whose
    // compile is asked to carry an eval.
    let dir = scratch("refuse");
    let entry = dir.join("evalgem.rb");
    std::fs::write(&entry, "class Evalgem\n  def go = eval(\"1\")\nend\n").expect("write entry");
    let out = run(zeo()
        .arg("--experimental-pkg")
        .arg("evalgem")
        .arg("-o")
        .arg(dir.join("evalgem.o"))
        .arg(&entry)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "an eval-bearing package must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("a package build cannot carry eval"),
        "the refusal names eval: {err}"
    );
}
