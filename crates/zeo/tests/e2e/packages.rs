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
if defined?(PURELEAF_TAG)
  p :guard_before_yes
else
  p :guard_before_no
end
p require "pureleaf"
# The branch below must SURVIVE the compile: the host arena has no writer
# of PURELEAF_TAG, only the package's manifest facts say one loads.
if defined?(PURELEAF_TAG)
  p :guard_after_yes
else
  p :guard_after_no
end
p defined?(Pureleaf::Deep)
p defined?(Pureleaf::Deep::WIDTH)
one = Pureleaf.new
p one.leaf
p one.tagged(2)
p Pureleaf.kind
p Pureleaf::Deep::WIDTH
p PURELEAF_TAG
p PURELEAF_PROBE
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
const WANT: &str = "nil\n:guard_before_no\ntrue\n:guard_after_yes\n\"constant\"\n\"constant\"\n\"leaf\"\n\"leaf-2\"\n:pure\n3\n7\n\"leaf-7\"\nfalse\n[:leaf, :tagged]\nNoMethodError\n";

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
fn every_exported_package_symbol_carries_the_package_prefix() {
    // The host's merged desc names a package's bodies, so they are EXPORTED
    // -- and an exported name without the prefix would collide the moment a
    // host (or a second package) defines the same Ruby class and method.
    let dir = scratch("symbols");
    let object = build_package(&dir);
    let out = run(Command::new("nm").arg("-gU").arg(&object));
    assert!(out.status.success(), "nm runs");
    let listing = String::from_utf8_lossy(&out.stdout);
    let stray: Vec<&str> = listing
        .lines()
        .filter_map(|l| l.split_whitespace().last())
        .filter(|sym| !sym.trim_start_matches('_').starts_with("zeo_pkg_pureleaf_"))
        .collect();
    assert!(
        stray.is_empty(),
        "every export is package-prefixed; strays: {stray:?}\n{listing}"
    );
}

#[test]
fn a_package_is_position_independent() {
    // The host defines its OWN classes before the require, shifting the
    // package's band -- the object was compiled knowing nothing of them,
    // and its id-translation table is what keeps every answer right.
    let dir = scratch("shift");
    let object = build_package(&dir);
    let host = dir.join("host.rb");
    // The nested-const guard runs PACKAGED-ONLY here: the eager-splice road
    // wrongly folds a pre-require `defined?(Pureleaf::Deep)` to "constant"
    // (a standing whole-program bug), while the interface's positional
    // classes answer ruby's nil.
    std::fs::write(
        &host,
        "class ShiftA\n  def a = 1\nend\nclass ShiftB\n  def b = 2\nend\n\
         p defined?(Pureleaf::Deep)\n\
         p ShiftA.new.a + ShiftB.new.b\nrequire \"pureleaf\"\n\
         p Pureleaf.new.tagged(9)\np Pureleaf.kind\np PURELEAF_PROBE\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--experimental-use-pkg")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    let out = ok(&mut Command::new(&bin));
    assert_eq!(out, "nil\n3\n\"leaf-9\"\n:pure\n\"leaf-7\"\n");
}

#[test]
fn two_packages_link_into_one_host() {
    // Both gems compiled ALONE claim overlapping local bands; the host
    // assigns each a disjoint final band and fills each object's own id
    // table and reveal stride.
    let dir = scratch("two");
    let leaf = build_package(&dir);
    let second = dir.join("puresecond.o");
    ok(zeo()
        .arg("--experimental-pkg")
        .arg("puresecond")
        .arg("-o")
        .arg(&second)
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/packages/puresecond/lib/puresecond.rb"),
        )
        .env("ZEO_CACHE", "0"));
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"pureleaf\"\nrequire \"puresecond\"\n\
         p Pureleaf.new.tagged(1)\np Puresecond.new.two\np Puresecond.kind\n\
         p [PURELEAF_TAG, PURESECOND_TAG]\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--experimental-use-pkg")
        .arg(&leaf)
        .arg("--experimental-use-pkg")
        .arg(&second)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    let out = ok(&mut Command::new(&bin));
    assert_eq!(out, "\"leaf-1\"\n22\n:second\n[7, 8]\n");
}

#[test]
fn the_manifest_interface_carries_clean_facts_and_typed_bodies() {
    // Two halves of compile-against-interface, pinned on the manifest text:
    // the unit-walk blanket must NOT leak into `patched_names` (a host would
    // refuse to devirtualize every packaged method), and each plain method
    // must name its exported body for the host's typed direct calls.
    let dir = scratch("manifest");
    let object = build_package(&dir);
    let zman =
        std::fs::read_to_string(object.with_extension("zman")).expect("read the manifest");
    assert!(
        zman.contains("\"patched_names\": []"),
        "the blanket de-opt stays out of the facts: {zman}"
    );
    assert!(
        zman.contains("zeo_pkg_pureleaf_m_Pureleaf_tagged"),
        "a plain method names its exported body: {zman}"
    );
}

/// The point of the interface: a host call site on a packaged receiver is
/// nominated, resolves the package's imported body, and emits the guarded
/// direct call. Read through `ZEO_DEBUG=trace-typed` -- a disassembly
/// cannot see an address-materialized call, and the trace names each stage
/// so a regression fails at the stage that broke.
#[test]
fn a_host_call_site_compiles_direct_into_the_package_body() {
    let dir = scratch("direct");
    let object = build_package(&dir);
    let src = dir.join("driver.rb");
    std::fs::write(
        &src,
        "require \"pureleaf\"\nclass Driver\n  def go\n    one = Pureleaf.new\n    one.tagged(3)\n  end\nend\np Driver.new.go\n",
    )
    .expect("write host");
    let bin = dir.join("driver");
    let out = run(zeo()
        .arg("--experimental-use-pkg")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&src)
        .env("ZEO_CACHE", "0")
        .env("ZEO_DEBUG", "trace-typed"));
    assert!(out.status.success(), "the driver host compiles");
    let trace = String::from_utf8_lossy(&out.stderr);
    assert!(
        trace.contains("extern typed method"),
        "the package's plain bodies were declared as imports: {trace}"
    );
    let emitted = trace
        .lines()
        .any(|l| l.contains("emitting direct call for tagged"));
    assert!(
        emitted,
        "the host method body's `one.tagged(3)` goes direct: {trace}"
    );
    assert_eq!(ok(&mut Command::new(&bin)), "\"leaf-3\"\n");
}

#[test]
fn a_host_reopen_of_a_package_class_is_refused_by_name() {
    let dir = scratch("reopen");
    let object = build_package(&dir);
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"pureleaf\"\nclass Pureleaf\n  def extra = 1\nend\n",
    )
    .expect("write host");
    let out = run(zeo()
        .arg("--experimental-use-pkg")
        .arg(&object)
        .arg("-o")
        .arg(dir.join("host-bin"))
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a host reopen must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("reopens `Pureleaf`"),
        "the refusal names the reopen: {err}"
    );
}

#[test]
fn a_host_subclass_of_a_package_class_is_refused_by_name() {
    let dir = scratch("subclass");
    let object = build_package(&dir);
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"pureleaf\"\nclass Sprout < Pureleaf\nend\np Sprout.new.leaf\n",
    )
    .expect("write host");
    let out = run(zeo()
        .arg("--experimental-use-pkg")
        .arg(&object)
        .arg("-o")
        .arg(dir.join("host-bin"))
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a host subclass must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("subclasses `Pureleaf`"),
        "the refusal names the subclass: {err}"
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

/// Compile any entry file as a package named `feature` into `dir`.
fn build_named_package(dir: &Path, feature: &str, entry: &Path) -> PathBuf {
    let object = dir.join(format!("{feature}.o"));
    ok(zeo()
        .arg("--experimental-pkg")
        .arg(feature)
        .arg("-o")
        .arg(&object)
        .arg(entry)
        .env("ZEO_CACHE", "0"));
    object
}

fn build_inline_package(dir: &Path, feature: &str, src: &str) -> PathBuf {
    let entry = dir.join(format!("{feature}.rb"));
    std::fs::write(&entry, src).expect("write package source");
    build_named_package(dir, feature, &entry)
}

/// Merge two packages into one host expecting a refusal; answer stderr.
fn refuse_merge(dir: &Path, a: &Path, b: &Path, host_src: &str) -> String {
    let host = dir.join("host.rb");
    std::fs::write(&host, host_src).expect("write host");
    let out = run(zeo()
        .arg("--experimental-use-pkg")
        .arg(a)
        .arg("--experimental-use-pkg")
        .arg(b)
        .arg("-o")
        .arg(dir.join("host-bin"))
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "the merge must refuse");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn two_packages_share_a_namespace_module() {
    // `module Sharedspace` in both gems: registration ALIASES the two
    // local ids onto one host id, the merged desc keeps one class row,
    // and both packages' methods land on it -- ruby's ordinary reopen,
    // across two precompiled objects. Verified against ruby 4.0.6.
    let dir = scratch("alias");
    let fixtures = fixture_gem().parent().expect("fixtures dir").to_path_buf();
    let alpha =
        build_named_package(&dir, "alphapart", &fixtures.join("alphapart/lib/alphapart.rb"));
    let beta = build_named_package(&dir, "betapart", &fixtures.join("betapart/lib/betapart.rb"));
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"alphapart\"\nrequire \"betapart\"\n\
         p Sharedspace::Alpha.new.a\np Sharedspace::Beta.new.b(3)\n\
         p [Sharedspace.alpha_tag, Sharedspace.beta_tag]\np Sharedspace::ALPHA_W\n",
    )
    .expect("write host");
    const ALIAS_WANT: &str = "\"alpha\"\n\"beta-3\"\n[:alpha, :beta]\n4\n";
    let packaged_bin = dir.join("host-packaged");
    ok(zeo()
        .arg("--experimental-use-pkg")
        .arg(&alpha)
        .arg("--experimental-use-pkg")
        .arg(&beta)
        .arg("-o")
        .arg(&packaged_bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&packaged_bin)),
        ALIAS_WANT,
        "the packaged program's answers"
    );
    let spliced_bin = dir.join("host-spliced");
    ok(zeo()
        .arg("--gems")
        .arg(&fixtures)
        .arg("-o")
        .arg(&spliced_bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&spliced_bin)),
        ALIAS_WANT,
        "the spliced program's answers"
    );
}

#[test]
fn two_packages_defining_one_method_on_a_shared_class_are_refused() {
    // A cross-package redefinition: the earlier package's typed sites
    // compiled against ITS body with no guard for a static replacement.
    // Patch rows lift this in M4; today it refuses by name.
    let dir = scratch("clash-method");
    let a = build_inline_package(&dir, "clasha", "module Shk\n  def self.tag = :a\nend\n");
    let b = build_inline_package(&dir, "clashb", "module Shk\n  def self.tag = :b\nend\n");
    let err = refuse_merge(&dir, &a, &b, "require \"clasha\"\nrequire \"clashb\"\np Shk.tag\n");
    assert!(
        err.contains("defines `Shk.tag`") && err.contains("another merged package"),
        "the refusal names the method: {err}"
    );
}

#[test]
fn a_shared_name_with_a_kind_mismatch_is_refused() {
    let dir = scratch("clash-kind");
    let a = build_inline_package(&dir, "kinda", "module Shp\n  def self.x = 1\nend\n");
    let b = build_inline_package(&dir, "kindb", "class Shp\n  def x = 1\nend\n");
    let err = refuse_merge(&dir, &a, &b, "require \"kinda\"\nrequire \"kindb\"\n");
    assert!(
        err.contains("defines `Shp` as a class") && err.contains("as a module"),
        "the refusal names the kind clash: {err}"
    );
}

#[test]
fn a_shared_class_with_two_ancestries_is_refused() {
    // Each package hangs the shared class off its OWN superclass; a chain
    // assembled from both would match neither compiled object.
    let dir = scratch("clash-parent");
    let a = build_inline_package(
        &dir,
        "parenta",
        "class Shqbase\nend\nclass Shqthing < Shqbase\n  def t = 1\nend\n",
    );
    let b = build_inline_package(
        &dir,
        "parentb",
        "class Shqother\nend\nclass Shqthing < Shqother\n  def u = 2\nend\n",
    );
    let err = refuse_merge(&dir, &a, &b, "require \"parenta\"\nrequire \"parentb\"\n");
    assert!(
        err.contains("reopens `Shqthing` with a different ancestry"),
        "the refusal names the ancestry clash: {err}"
    );
}

#[test]
fn a_package_defining_a_top_level_method_is_refused_today() {
    // Object is the ONE class a package and a host share with no id
    // boundary, so a package's top-level def could interleave with a
    // host def of the same name -- and no manifest fact carries that
    // today (the unit-blanket split deliberately keeps packaged names
    // out of `patched_names`). The value-channel refusal is what keeps
    // the shape unreachable; lifting it (M4) must revisit the Object
    // channel's facts.
    let dir = scratch("toplevel");
    let entry = dir.join("topgem.rb");
    std::fs::write(
        &entry,
        "def shade = :package\nclass Topgem\n  def call_shade = shade\nend\n",
    )
    .expect("write entry");
    let out = run(zeo()
        .arg("--experimental-pkg")
        .arg("topgem")
        .arg("-o")
        .arg(dir.join("topgem.o"))
        .arg(&entry)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a top-level def must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("a package build cannot carry a builtin reopen"),
        "the refusal names the channel: {err}"
    );
}
