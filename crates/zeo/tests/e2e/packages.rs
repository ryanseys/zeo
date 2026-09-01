//! Separate compilation's merge spine.
//!
//! A gem compiles ONCE to its own object plus a row manifest
//! (`--package`); a host program merges the manifest into its one
//! `ProgramDesc` and links the object beside its own
//! (`--with-package`). These tests hold the two contracts the
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
        .arg("--package")
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
        .arg("--with-package")
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
        .arg("--with-package")
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
        .arg("--package")
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
        .arg("--with-package")
        .arg(&leaf)
        .arg("--with-package")
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
        .arg("--with-package")
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
fn a_host_reopen_of_a_package_class_installs_at_its_position() {
    // The package's rows travel in its compiled object, so the host's
    // reopen installs at RUN time, at its document position: before the
    // reopen the package body answers, after it the host's -- ruby's
    // install-where-it-stands, across the package boundary. The install
    // patches the class, which deoptimizes the package's own guarded
    // sites. Verified against ruby 4.0.6.
    let dir = scratch("reopen");
    let object = build_inline_package(
        &dir,
        "leafgem",
        "class Leafgem\n  def tag = :original\n  def stable = :stable\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"leafgem\"\none = Leafgem.new\np one.tag\np defined?(one.extra)\n\
         class Leafgem\n  def tag = :patched\n  def extra = :extra\nend\n\
         p one.tag\np one.extra\np one.stable\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        ":original\nnil\n:patched\n:extra\n:stable\n"
    );
}

#[test]
fn a_host_subclass_of_a_package_class_inherits_its_surface() {
    // The child's rows name the package's exported trampolines; its ivar
    // layout starts with the parent's manifest list verbatim (the package
    // bodies bake those slots); `super` reaches the package body; and the
    // inherited class method rides the flattened class-method channel.
    // Verified against ruby 4.0.6.
    let dir = scratch("subclass");
    let object = build_inline_package(
        &dir,
        "leafgem2",
        "class Leafgem2\n  def initialize(n = 5)\n    @n = n\n  end\n  def n = @n\n  def tag = :original\n  def self.kind = :leaf_kind\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"leafgem2\"\n\
         class Sprout < Leafgem2\n  def initialize\n    super(7)\n    @extra2 = 1\n  end\n  def tag = [:sprouted, super]\n  def both = [@n, @extra2]\nend\n\
         p Leafgem2.new.n\ns = Sprout.new\np s.n\np s.both\np s.tag\np Sprout.kind\n\
         p Sprout.instance_methods(false).sort\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        "5\n7\n[7, 1]\n[:sprouted, :original]\n:leaf_kind\n[:both, :tag]\n"
    );
}

#[test]
fn a_package_carrying_a_box_is_refused_by_name() {
    // Box ids cross the boundary in code and in rows; until they rebase
    // through the stride array, a box-bearing package refuses loudly.
    let dir = scratch("refuse");
    let entry = dir.join("boxgem.rb");
    std::fs::write(
        &entry,
        "box = Ruby::Box.new\nbox.eval_string(\"X = 1\")\nclass Boxgem\nend\n",
    )
    .expect("write entry");
    let out = run(zeo()
        .arg("--package")
        .arg("boxgem")
        .arg("-o")
        .arg(dir.join("boxgem.o"))
        .arg(&entry)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a box-bearing package must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("a package build cannot carry Ruby::Box"),
        "the refusal names the box: {err}"
    );
}

#[test]
fn a_packages_redefinition_timeline_survives_the_boundary() {
    // Each package's redefinition rows sit at the front of the merged
    // reflection table at a link-time stride; the mid-body call answers
    // the FIRST body, later calls the second, and `source_location`
    // proves the SECOND package's rows resolve past the first's.
    // Verified against ruby 4.0.6.
    let dir = scratch("redef");
    let one = build_inline_package(
        &dir,
        "redefgem",
        "class Redefgem\n  def face = :first\n  FIRST = Redefgem.new.face\n  def face(n = 0) = [:second, n]\nend\n",
    );
    let two = build_inline_package(
        &dir,
        "redefgem2",
        "class Redefgem2\n  def voice = :quiet\n  EARLY = Redefgem2.new.voice\n  def voice = :loud\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"redefgem\"\nrequire \"redefgem2\"\n\
         p Redefgem::FIRST\np Redefgem2::EARLY\n\
         p Redefgem.new.face(2)\np Redefgem2.new.voice\n\
         p Redefgem.new.method(:face).arity\n\
         p Redefgem2.instance_method(:voice).source_location&.last\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&one)
        .arg("--with-package")
        .arg(&two)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        ":first\n:quiet\n[:second, 2]\n:loud\n-1\n4\n"
    );
}

#[test]
fn a_host_includes_and_extends_a_packaged_module() {
    // A module's instance methods ride the manifest's value-channel rows;
    // the host's includer flattens them onto itself through the exported
    // trampolines, and the module body's own sends resolve by name into
    // the HOST's methods (`greet` reads `label`, which only the host
    // defines). `extend` is the class-side twin. Verified against ruby
    // 4.0.6.
    let dir = scratch("mixin");
    let object = build_inline_package(
        &dir,
        "modgem",
        "module Modgem\n  def greet = \"hello #{label}\"\n  def shout = greet.upcase\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"modgem\"\n\
         class Wearer\n  include Modgem\n  def label = \"wearer\"\nend\n\
         class Toolbox\n  extend Modgem\n  def self.label = \"toolbox\"\nend\n\
         w = Wearer.new\np w.greet\np w.shout\n\
         p Wearer.ancestors.first(2).map(&:name)\np Wearer.include?(Modgem)\n\
         p Toolbox.greet\np Toolbox.shout\n\
         p Toolbox.singleton_class.include?(Modgem)\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        "\"hello wearer\"\n\"HELLO WEARER\"\n[\"Wearer\", \"Modgem\"]\ntrue\n\
         \"hello toolbox\"\n\"HELLO TOOLBOX\"\ntrue\n"
    );
}

#[test]
fn a_hosts_global_def_hook_hears_a_packages_definitions() {
    // The package compiled with no hook in sight, so each of its
    // definitions announces through the run-time probe; the host's
    // `method_added`/`singleton_method_added` bodies answer, in ruby's
    // order (each hook announces itself first). Verified against ruby
    // 4.0.6.
    let dir = scratch("defhook");
    let object = build_inline_package(
        &dir,
        "hookgem",
        "class Hookgem\n  def alpha = 1\n  def self.side = 2\n  def omega = 3\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "class Module\n  def method_added(n) = puts(\"added #{n}\")\nend\n\
         class BasicObject\n  def singleton_method_added(n) = puts(\"s-added #{n}\")\nend\n\
         require \"hookgem\"\np Hookgem.new.alpha\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        "added method_added\nadded singleton_method_added\nadded alpha\n\
         s-added side\nadded omega\n1\n"
    );
}

#[test]
fn an_eval_bearing_package_links_against_the_hosts_compiler() {
    // The package's manifest says it compiles at run time; the HOST has no
    // eval of its own, so only the fact union makes it embed the run-time
    // compiler. The snippet reads a captured local, resolves the package's
    // own class through the runtime registry, and installs a method the
    // host then calls dynamically.
    let dir = scratch("evalpkg");
    let object = build_inline_package(
        &dir,
        "evalgem",
        "class Evalgem\n  def go(n) = eval(\"n * 3\")\n  def kls = eval(\"Evalgem\").name\n  def mint\n    Evalgem.class_eval(\"def minted = :minted\")\n  end\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"evalgem\"\ne = Evalgem.new\np e.go(4)\np e.kls\ne.mint\np e.minted\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(ok(&mut Command::new(&bin)), "12\n\"Evalgem\"\n:minted\n");
}

/// Compile any entry file as a package named `feature` into `dir`.
fn build_named_package(dir: &Path, feature: &str, entry: &Path) -> PathBuf {
    let object = dir.join(format!("{feature}.o"));
    ok(zeo()
        .arg("--package")
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
        .arg("--with-package")
        .arg(a)
        .arg("--with-package")
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
        .arg("--with-package")
        .arg(&alpha)
        .arg("--with-package")
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
fn regexp_and_flip_flop_sites_get_disjoint_strides() {
    // Both packages mint their FIRST regexp literal as local site 0; the
    // runtime caches one frozen regexp per site id, so a collided id would
    // answer the first package's pattern for the second's literal. The
    // stride array keeps the spaces disjoint, and the host's own literal
    // sits past both. Flip-flop latches ride the same rule. Verified
    // against ruby 4.0.6.
    let dir = scratch("strides");
    let regexgem = build_inline_package(
        &dir,
        "regexgem",
        "class Regexgem\n  def pat = /alpha[0-9]+/.source\n  def bees(s) = s.scan(/b+/).length\nend\n",
    );
    let flipgem = build_inline_package(
        &dir,
        "flipgem",
        "class Flipgem\n  def mark = /zeta/.source\n  def spans(xs)\n    out = []\n    i = 0\n    while i < xs.length\n      x = xs[i]\n      out << x if (x == 1)..(x == 3)\n      i += 1\n    end\n    out\n  end\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"regexgem\"\nrequire \"flipgem\"\n\
         p Regexgem.new.pat\np Flipgem.new.mark\n\
         p Regexgem.new.bees(\"abbba bb\")\n\
         p Flipgem.new.spans([0, 1, 2, 3, 4, 1, 9, 3, 5])\n\
         p((/hostpat/).source)\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&regexgem)
        .arg("--with-package")
        .arg(&flipgem)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        "\"alpha[0-9]+\"\n\"zeta\"\n2\n[1, 2, 3, 1, 9, 3]\n\"hostpat\"\n"
    );
}

#[test]
fn two_packages_defining_one_method_on_a_shared_class_are_refused() {
    // A cross-package redefinition: the earlier package's typed sites
    // compiled against ITS body with no guard for a static replacement.
    // Patch rows will lift this; today it refuses by name.
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
fn a_packages_top_level_def_reaches_the_host() {
    // Object is the one class a package and a host share with no id
    // boundary. The package's top-level def travels as a value-channel
    // row on it, and the package's OWN interior call goes through
    // dispatch rather than binding its body -- this tier carries no
    // guard, so a direct call could never see a redefinition. A HOST
    // static def of the same name refuses by package name (two static
    // bodies for one shared-class name), which the drop-to-splice tier
    // turns into a source recompile.
    let dir = scratch("toplevel");
    let object = build_inline_package(
        &dir,
        "topgem",
        "def shade = :package\nclass Topgem\n  def call_shade = shade\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(&host, "require \"topgem\"\np shade\np Topgem.new.call_shade\n")
        .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(ok(&mut Command::new(&bin)), ":package\n:package\n");

    let collide = dir.join("collide.rb");
    std::fs::write(
        &collide,
        "require \"topgem\"\ndef shade = :host\np shade\n",
    )
    .expect("write collide host");
    let out = run(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(dir.join("collide-bin"))
        .arg(&collide)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a colliding host def must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("package 'topgem' also defines"),
        "the refusal names the package: {err}"
    );
}

#[test]
fn a_package_reopens_builtins_and_the_host_sees_every_road() {
    // The widest builtin-reopen surface in one program: a Kernel module
    // method (receiverless AND explicit-receiver), a NEW String method, a
    // REDEFINED native (`String#length`), and a redefined `Array#map` that
    // a host block call must reach -- the last is the iterator-fusion
    // suppression working through the package's interface. Verified
    // against ruby 4.0.6.
    let dir = scratch("breopen");
    let object = build_inline_package(
        &dir,
        "kernelgem",
        "module Kernel\n  def khelper(x) = [:k, x]\nend\n\
         def toplevel_helper(y) = [:top, y]\n\
         class String\n  def shout = upcase + \"!\"\n  def length = 999\nend\n\
         class Array\n  def map = :hijacked\nend\n\
         class Kernelgem\nend\n",
    );
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"kernelgem\"\np khelper(1)\np 5.khelper(2)\np toplevel_helper(3)\n\
         p \"hi\".shout\np \"abcd\".length\n\
         xs = [1, 2, 3]\np xs.map { |v| v * 2 }\np xs.map\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&object)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        "[:k, 1]\n[:k, 2]\n[:top, 3]\n\"HI!\"\n999\n:hijacked\n:hijacked\n"
    );
}

#[test]
fn a_zeopkg_bundle_builds_links_and_runs() {
    // `-o pkg.zeopkg` bundles the object and the manifest into ONE file --
    // the shippable artifact -- and a host consumes it exactly like the
    // two-file spelling.
    let dir = scratch("bundle");
    let artifact = dir.join("pureleaf.zeopkg");
    ok(zeo()
        .arg("--package")
        .arg("pureleaf")
        .arg("-o")
        .arg(&artifact)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    assert!(artifact.is_file(), "the bundle was written");
    assert!(
        !artifact.with_extension("zman").is_file(),
        "the bundle carries the manifest inside; no loose copy remains"
    );
    let host = dir.join("host.rb");
    std::fs::write(&host, "require \"pureleaf\"\np Pureleaf.new.tagged(5)\n").expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&artifact)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(ok(&mut Command::new(&bin)), "\"leaf-5\"\n");
}

#[test]
fn a_package_build_is_reproducible_across_directories() {
    // Decision 6, CI-asserted: the same gem compiled from two different
    // checkouts answers byte-identical artifacts. The virtual-root
    // respelling is what keeps the build directory out of the object's
    // rodata and the manifest's meta rows.
    let dir = scratch("repro");
    for side in ["one", "two"] {
        let copy = dir.join(side);
        std::fs::create_dir_all(copy.join("lib")).expect("mkdir");
        std::fs::copy(
            fixture_gem().join("lib/pureleaf.rb"),
            copy.join("lib/pureleaf.rb"),
        )
        .expect("copy the gem source");
        ok(zeo()
            .arg("--package")
            .arg("pureleaf")
            .arg("-o")
            .arg(dir.join(format!("{side}.zeopkg")))
            .arg(copy.join("lib/pureleaf.rb"))
            .env("ZEO_CACHE", "0"));
    }
    let one = std::fs::read(dir.join("one.zeopkg")).expect("read one");
    let two = std::fs::read(dir.join("two.zeopkg")).expect("read two");
    assert!(one == two, "the two builds are byte-identical");
}

#[test]
fn the_package_cache_serves_hits_and_invalidates_on_edit() {
    // The machine-wide package cache, proven the only way a deterministic
    // compiler can be: MARK the cached artifact, and the next build serves
    // the mark; edit the source, and the build stops serving it.
    let dir = scratch("pkgcache");
    let cache = dir.join("cache");
    // The source sits in its own directory: the cache manifest stamps the
    // directories a compile read, so an output landing beside the source
    // would read as a change.
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    let entry = src.join("cachegem.rb");
    std::fs::write(&entry, "class Cachegem\n  def go = :one\nend\n").expect("write the gem");
    let build = |out: &Path| {
        ok(zeo()
            .arg("--package")
            .arg("cachegem")
            .arg("-o")
            .arg(out)
            .arg(&entry)
            .env("ZEO_CACHE", "1")
            .env("ZEO_PACKAGE_CACHE", &cache));
    };
    build(&dir.join("first.zeopkg"));
    let cached = std::fs::read_dir(&cache)
        .expect("the cache dir exists")
        .filter_map(Result::ok)
        .map(|e| e.path().join("pkg.zeopkg"))
        .find(|p| p.is_file())
        .expect("one cache entry");
    let (manifest, object) = zeo::package::read_zeopkg(&cached).expect("read the entry");
    let marked = manifest.replace("\"iface_hash\": \"", "\"iface_hash\": \"cafe");
    assert_ne!(marked, manifest, "the mark landed");
    zeo::package::write_zeopkg(&cached, &marked, &object).expect("mark the entry");

    build(&dir.join("second.zeopkg"));
    let (manifest, _) =
        zeo::package::read_zeopkg(&dir.join("second.zeopkg")).expect("read the second build");
    let listing: Vec<String> = std::fs::read_dir(&cache)
        .map(|es| {
            es.filter_map(Result::ok)
                .map(|e| {
                    let names: Vec<String> = std::fs::read_dir(e.path())
                        .map(|fs| {
                            fs.filter_map(Result::ok)
                                .map(|f| f.file_name().to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    let rows = std::fs::read_to_string(e.path().join("manifest"))
                        .unwrap_or_default();
                    format!("{}: {names:?} rows: {rows}", e.file_name().to_string_lossy())
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(
        manifest.contains("\"iface_hash\": \"cafe"),
        "the second build was served from the cache; cache dirs: {listing:?}"
    );

    std::fs::write(&entry, "class Cachegem\n  def go = :two\nend\n").expect("edit the gem");
    build(&dir.join("third.zeopkg"));
    let (manifest, _) =
        zeo::package::read_zeopkg(&dir.join("third.zeopkg")).expect("read the third build");
    assert!(
        !manifest.contains("\"iface_hash\": \"cafe"),
        "the source edit invalidated the entry"
    );
}

#[test]
fn a_refused_artifact_drops_to_the_source_splice() {
    // The fallback tier: the same foreign-target artifact that REFUSES
    // when it is the only copy compiles from source when the gem is
    // resolvable -- with a warning naming the package, never silently.
    // (A host reopen no longer triggers a drop: it installs at run time
    // and composes with the artifact.)
    let dir = scratch("drop");
    let artifact = dir.join("pureleaf.zeopkg");
    ok(zeo()
        .arg("--package")
        .arg("pureleaf")
        .arg("-o")
        .arg(&artifact)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    let (manifest, object) = zeo::package::read_zeopkg(&artifact).expect("read");
    let foreign = manifest.replace(
        &format!("\"target\": \"{}\"", target_of(&manifest)),
        "\"target\": \"wasm32-unknown-unknown\"",
    );
    assert_ne!(foreign, manifest, "the target was rewritten");
    zeo::package::write_zeopkg(&artifact, &foreign, &object).expect("rewrite");
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"pureleaf\"\nclass Pureleaf\n  def extra = :host_extra\nend\n\
         p Pureleaf.new.extra\np Pureleaf.new.tagged(1)\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    let out = run(zeo()
        .arg("--with-package")
        .arg(&artifact)
        .arg("--gems")
        .arg(fixture_gem().parent().expect("fixtures dir"))
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(out.status.success(), "the drop compiles from source");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("dropping the precompiled artifact for 'pureleaf'"),
        "the drop warns by name: {err}"
    );
    assert_eq!(ok(&mut Command::new(&bin)), ":host_extra\n\"leaf-1\"\n");
}

#[test]
fn a_target_mismatch_refuses_by_name() {
    // No stable package ABI is promised: the identity contract is an exact
    // match, and a mismatch names itself instead of surfacing as a link
    // error.
    let dir = scratch("target");
    let artifact = dir.join("pureleaf.zeopkg");
    ok(zeo()
        .arg("--package")
        .arg("pureleaf")
        .arg("-o")
        .arg(&artifact)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    let (manifest, object) = zeo::package::read_zeopkg(&artifact).expect("read");
    let foreign = manifest.replace(
        &format!("\"target\": \"{}\"", target_of(&manifest)),
        "\"target\": \"wasm32-unknown-unknown\"",
    );
    assert_ne!(foreign, manifest, "the target was rewritten");
    zeo::package::write_zeopkg(&artifact, &foreign, &object).expect("rewrite");
    let host = dir.join("host.rb");
    std::fs::write(&host, "require \"pureleaf\"\n").expect("write host");
    let out = run(zeo()
        .arg("--with-package")
        .arg(&artifact)
        .arg("-o")
        .arg(dir.join("host-bin"))
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a foreign target must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("was compiled for wasm32-unknown-unknown"),
        "the refusal names both targets: {err}"
    );
}

/// The `target` value inside a manifest's JSON text.
fn target_of(manifest: &str) -> String {
    serde_json::from_str::<serde_json::Value>(manifest)
        .expect("a manifest parses")
        .get("target")
        .and_then(|t| t.as_str())
        .expect("a manifest names its target")
        .to_string()
}

/// The store tier end to end: `zeo install` precompiles a locked gem into
/// the gem store, and a store-active compile LINKS that artifact instead
/// of splicing the gem's source. The proof is a poisoned source tree --
/// only the artifact carries the working body, so the answer can only
/// come from the link. Removing the artifact flips the same compile back
/// to the source splice, which is the fallback contract.
/// A one-gem project against a scratch store, RubyGems-shaped: the
/// `tinygem` sources, its gemspec, and a locked Gemfile beside an app
/// that requires it. Answers `(store, proj)`.
fn tinygem_project(dir: &Path) -> (PathBuf, PathBuf) {
    let store = dir.join("store");
    let gem_lib = store.join("gems/tinygem-1.0.0/lib");
    std::fs::create_dir_all(store.join("specifications")).expect("mkdir");
    std::fs::create_dir_all(&gem_lib).expect("mkdir");
    std::fs::write(
        store.join("specifications/tinygem-1.0.0.gemspec"),
        "Gem::Specification.new do |s|\n  s.name = \"tinygem\"\n  s.version = \"1.0.0\"\n  \
         s.require_paths = [\"lib\"]\nend\n",
    )
    .expect("write gemspec");
    let good = "class Tinygem\n  def greet = \"hello from tinygem\"\nend\n";
    std::fs::write(gem_lib.join("tinygem.rb"), good).expect("write lib");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).expect("mkdir");
    std::fs::write(
        proj.join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"tinygem\"\n",
    )
    .expect("write Gemfile");
    std::fs::write(
        proj.join("Gemfile.lock"),
        "GEM\n  remote: https://rubygems.org/\n  specs:\n    tinygem (1.0.0)\n\nPLATFORMS\n  \
         ruby\n\nDEPENDENCIES\n  tinygem\n\nBUNDLED WITH\n   4.0.18\n",
    )
    .expect("write lock");
    std::fs::write(proj.join("app.rb"), "require \"tinygem\"\nputs Tinygem.new.greet\n")
        .expect("write app");
    (store, proj)
}

#[test]
fn zeo_install_populates_the_store_and_a_compile_links_it() {
    let dir = scratch("install");
    let (store, proj) = tinygem_project(&dir);
    let gem_lib = store.join("gems/tinygem-1.0.0/lib");

    let report = ok(zeo()
        .current_dir(&proj)
        .arg("install")
        .arg("--gem-path")
        .arg(&store)
        .env("ZEO_CACHE", "0"));
    assert!(report.contains("install tinygem 1.0.0"), "report: {report}");
    let artifacts: Vec<PathBuf> = walk_files(&store.join("zeo"));
    assert_eq!(
        artifacts.len(),
        1,
        "one artifact at its store home: {artifacts:?}"
    );
    assert!(artifacts[0].ends_with("tinygem-1.0.0/pkg.zeopkg"));

    // Only the ARTIFACT has the working body now.
    std::fs::write(gem_lib.join("tinygem.rb"), "raise \"the SOURCE was spliced\"\n")
        .expect("poison lib");
    let compile = |out: &str| {
        let bin = proj.join(out);
        zeo()
            .current_dir(&proj)
            .arg("build")
            .arg("app.rb")
            .arg("--gem-path")
            .arg(&store)
            .arg("--bundle-gemfile")
            .arg("Gemfile")
            .arg("-o")
            .arg(&bin)
            .env("ZEO_CACHE", "0")
            .output()
            .expect("spawn");
        bin
    };
    let linked = compile("app-linked");
    let out = Command::new(&linked).output().expect("run");
    assert!(
        out.status.success(),
        "the artifact answers: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from tinygem\n");

    // No artifact -> the same compile splices the source again.
    std::fs::remove_file(&artifacts[0]).expect("remove artifact");
    let spliced = compile("app-spliced");
    let out = Command::new(&spliced).output().expect("run");
    assert!(!out.status.success(), "the poisoned source raises");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("the SOURCE was spliced"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `zeo flags` prints the handoff line the compile itself consumes: the
/// store, the Gemfile, and -- once `zeo install` has run -- the artifact.
/// The line is shell-quoted for `$(zeo flags)`; `--json` is the tools'
/// form and carries the same artifact path.
#[test]
fn zeo_flags_prints_the_projects_handoff_line() {
    let dir = scratch("flags");
    let (store, proj) = tinygem_project(&dir);

    let before = ok(zeo()
        .current_dir(&proj)
        .arg("flags")
        .arg("--gem-path")
        .arg(&store));
    assert!(before.contains("--gem-path '"), "line: {before}");
    assert!(before.contains("--bundle-gemfile '"), "line: {before}");
    assert!(
        !before.contains("--with-package"),
        "no artifact before install: {before}"
    );

    ok(zeo()
        .current_dir(&proj)
        .arg("install")
        .arg("--gem-path")
        .arg(&store)
        .env("ZEO_CACHE", "0"));
    let after = ok(zeo()
        .current_dir(&proj)
        .arg("flags")
        .arg("--gem-path")
        .arg(&store));
    assert!(
        after.contains("--with-package '") && after.contains("tinygem-1.0.0/pkg.zeopkg'"),
        "the installed artifact rides the line: {after}"
    );
    let json = ok(zeo()
        .current_dir(&proj)
        .arg("flags")
        .arg("--json")
        .arg("--gem-path")
        .arg(&store));
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    let artifact = parsed["gems"][0]["artifact"].as_str().expect("an artifact");
    assert!(artifact.ends_with("tinygem-1.0.0/pkg.zeopkg"), "{artifact}");
}

/// Every file under `root`, recursively.
fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// The shipping tier end to end: `zeo gem precompile` builds a platform
/// gem with the artifact inside (through RubyGems' own Gem::Package),
/// `zeo gem install` unpacks it into a store, `zeo install` PROMOTES the
/// shipped artifact instead of compiling, and a store-active compile
/// links it. The greeting names the artifact so a source splice cannot
/// fake the answer once the tree is poisoned.
#[test]
fn a_precompiled_platform_gem_ships_its_artifact_to_a_consumer() {
    let dir = scratch("shipping");
    let gem_dir = dir.join("shipgem");
    std::fs::create_dir_all(gem_dir.join("lib")).expect("mkdir");
    std::fs::write(
        gem_dir.join("shipgem.gemspec"),
        "Gem::Specification.new do |s|\n  s.name = \"shipgem\"\n  s.version = \"2.0.0\"\n  \
         s.summary = \"ships an artifact\"\n  s.authors = [\"e2e\"]\n  \
         s.files = [\"lib/shipgem.rb\"]\n  s.require_paths = [\"lib\"]\nend\n",
    )
    .expect("write gemspec");
    std::fs::write(
        gem_dir.join("lib/shipgem.rb"),
        "class Shipgem\n  def greet = \"hello from the shipped artifact\"\nend\n",
    )
    .expect("write lib");
    let report = ok(zeo()
        .current_dir(&gem_dir)
        .arg("gem")
        .arg("precompile")
        .env("ZEO_CACHE", "0"));
    assert!(
        report.contains("carries the precompiled artifact"),
        "report: {report}"
    );
    let gems: Vec<PathBuf> = std::fs::read_dir(&gem_dir)
        .expect("read gem dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "gem"))
        .collect();
    assert_eq!(gems.len(), 1, "one platform gem: {gems:?}");
    assert!(!gem_dir.join("zeo").exists(), "the loose artifact is cleaned");

    // The consumer machine: install the .gem, then let `zeo install`
    // promote the shipped artifact -- no compile.
    let store = dir.join("store");
    ok(zeo()
        .arg("gem")
        .arg("install")
        .arg("--local")
        .arg("--install-dir")
        .arg(&store)
        .arg(&gems[0]));
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).expect("mkdir");
    std::fs::write(
        proj.join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"shipgem\"\n",
    )
    .expect("write Gemfile");
    std::fs::write(
        proj.join("Gemfile.lock"),
        "GEM\n  remote: https://rubygems.org/\n  specs:\n    shipgem (2.0.0)\n\nPLATFORMS\n  \
         ruby\n\nDEPENDENCIES\n  shipgem\n\nBUNDLED WITH\n   4.0.18\n",
    )
    .expect("write lock");
    std::fs::write(proj.join("app.rb"), "require \"shipgem\"\nputs Shipgem.new.greet\n")
        .expect("write app");
    let report = ok(zeo()
        .current_dir(&proj)
        .arg("install")
        .arg("--gem-path")
        .arg(&store)
        .env("ZEO_CACHE", "0"));
    assert!(
        report.contains("shipped artifact"),
        "the artifact promotes without a compile: {report}"
    );

    // Poison the installed SOURCE: only the artifact can answer now.
    let installed_lib = store.join("gems").join(
        std::fs::read_dir(store.join("gems"))
            .expect("read store gems")
            .flatten()
            .map(|e| e.file_name())
            .find(|n| n.to_string_lossy().starts_with("shipgem-"))
            .expect("the installed gem dir"),
    );
    std::fs::write(
        installed_lib.join("lib/shipgem.rb"),
        "raise \"the SOURCE was spliced\"\n",
    )
    .expect("poison lib");
    let bin = proj.join("app-bin");
    ok(zeo()
        .current_dir(&proj)
        .arg("build")
        .arg("app.rb")
        .arg("--gem-path")
        .arg(&store)
        .arg("--bundle-gemfile")
        .arg("Gemfile")
        .arg("-o")
        .arg(&bin)
        .env("ZEO_CACHE", "0"));
    let out = Command::new(&bin).output().expect("run");
    assert!(
        out.status.success(),
        "the shipped artifact answers: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hello from the shipped artifact\n"
    );
}
