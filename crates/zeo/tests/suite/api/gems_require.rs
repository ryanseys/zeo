use crate::support::{
    compile_packages, compile_project, compile_project_strict, run_ruby, run_ruby_packages,
};

#[test]
fn gem_disclosure_report_records_how_each_library_was_satisfied() {
    // The report is the honesty anchor for the compatibility claim,
    // so it gets a test that fails when it lies. Runs on the DEFAULT (report-on)
    // path -- the thing --no-report suppresses -- not the harness opt-out.
    let report = std::env::temp_dir().join(format!("zeo-gems-test-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&report);
    let opts = zeo::CompileOptions {
        gem_report: Some(report.clone()),
        ..Default::default()
    };
    zeo::check_program_with(
        "require \"json\"\nrequire \"optparse\"\nrequire \"base64\"\nputs 1\n",
        &opts,
    )
    .expect("compiles");
    let json = std::fs::read_to_string(&report).expect("report was written");
    let _ = std::fs::remove_file(&report);

    // A gem with a Ruby half, over a divergent native backing.
    assert!(json.contains(r#""json": {"by": "bundled-gem""#), "{json}");
    assert!(json.contains("serde_json-backed"), "{json}");
    // A faithful pure-Ruby bundled gem: bundled-gem, NOT flagged divergent.
    assert!(
        json.contains(r#""optparse": {"by": "bundled-gem""#),
        "{json}"
    );
    assert!(
        !json[json.find("\"optparse\"").unwrap()..]
            .lines()
            .next()
            .unwrap()
            .contains("diverges"),
        "optparse must not be marked divergent: {json}"
    );
    // A directly-required static ext: builtin-ext, divergent.
    assert!(
        json.contains(r#""base64": {"by": "builtin-ext", "feature": "base64", "diverges": true"#),
        "{json}"
    );
}

#[test]
fn gem_compat_classifies_each_locked_gem() {
    // The LAYOUT classification `zeo::gem_compat` answers, over the
    // self-contained fixture store: pure Ruby compiles, a gem shipping its C
    // as SOURCE is native-source, and a precompiled-only gem is the one that
    // stays unsupported.
    use zeo::GemCompatOutcome;
    let store = crate::support::gem_store("compat");
    let entries = zeo::gem_compat(&store, &store.join("Gemfile.lock")).unwrap();
    let outcome = |n: &str| {
        entries
            .iter()
            .find(|e| e.name == n)
            .unwrap()
            .outcome
            .clone()
    };

    assert_eq!(outcome("purelib"), GemCompatOutcome::Compiled);
    assert!(
        matches!(
            outcome("nativelib"),
            GemCompatOutcome::NativeSource { ref extensions }
                if extensions == &["ext/nativelib/extconf.rb"]
        ),
        "{:?}",
        outcome("nativelib")
    );
    assert!(matches!(
        outcome("precompiled"),
        GemCompatOutcome::NativeUnsupported { ref kind, .. } if kind == "precompiled-platform-gem"
    ));

    // The no-lockfile mode sweeps the store's specifications/ directly and
    // classifies the same three gems (the broad out-of-the-box sample).
    let swept = zeo::gem_compat_installed(&store).unwrap();
    let sweep_outcome = |n: &str| {
        swept
            .iter()
            .find(|e| e.name == n)
            .map(|e| e.outcome.clone())
    };
    assert_eq!(sweep_outcome("purelib"), Some(GemCompatOutcome::Compiled));
    assert!(matches!(
        sweep_outcome("nativelib"),
        Some(GemCompatOutcome::NativeSource { .. })
    ));
    assert!(matches!(
        sweep_outcome("precompiled"),
        Some(GemCompatOutcome::NativeUnsupported { .. })
    ));
}

// ---- require/require_relative/load compile-time splicing ----
// Every positive-path expectation below was oracle-verified against real
// `ruby` (4.0.6) first, per this project's standing convention.

#[test]
fn autoload_with_a_dynamic_feature_defers_to_the_runtime_row() {
    // A target the compile-time splice cannot name is no longer a rejection:
    // it lowers to a real `Module#autoload` call, which resolves the string the
    // program actually builds against the load path compiled in for it
    // (`zeo_rt::features`). Compiling is the assertion -- what the call then
    // finds is covered by `tests/autoload_dsl_computes_its_own_path.rb`.
    assert!(
        zeo::check_program("autoload :X, some_method_call").is_ok(),
        "a computed autoload target should compile and defer to the runtime"
    );
}

#[test]
fn a_plain_missing_require_defers_to_a_runtime_load_error() {
    // A plain `require "feature"` zeo can't resolve is NOT a compile error under
    // whole-program AOT: it lowers to a runtime `Kernel#require` (raising CRuby's
    // `cannot load such file -- definitely_missing` LoadError), so it COMPILES.
    // The runtime crash / rescue behavior is covered by
    // `requires_that_cannot_be_resolved_at_compile_time`.
    assert!(
        compile_project(
            &[("main.rb", "require \"definitely_missing\"\n")],
            "main.rb",
            &[],
        )
        .is_ok(),
        "a missing plain require should compile (defers to runtime)"
    );
}

#[test]
fn ripper_is_declined_and_the_load_error_says_so() {
    // A DECLINE, not a gap: ripper exposes the reduction event stream of
    // CRuby's `parse.y`, and zeo's front end embeds prism -- a different parser
    // with a different event model -- so there is nothing for a binding to bind
    // to. Matching ripper means re-implementing CRuby's grammar actions.
    //
    // Asserted here rather than as a golden because a golden is diffed against
    // the ruby oracle, and ruby loads ripper happily: the two can never agree,
    // so there is no passing golden to write. This is the shape the gaps README
    // means by "a divergence zeo has decided not to reproduce belongs in a
    // passing test that documents it".
    //
    // The message must NAME the decision. A bare `cannot load such file --
    // ripper` reads like a typo or an unfinished feature, and sends the caller
    // looking for a version of zeo that has it.
    let result = run_ruby("require \"ripper\"\np defined?(Ripper)\n");
    assert!(
        !result.status.success(),
        "expected `require \"ripper\"` to raise; stdout: {}",
        result.stdout
    );
    assert!(
        result.stderr.contains("cannot load such file -- ripper")
            && result.stderr.contains("declined")
            && result.stderr.contains("prism")
            && result.stderr.contains("LoadError"),
        "the decline must explain itself: {}",
        result.stderr
    );

    // It is a plain LoadError, so the optional-dependency idiom still works --
    // a caller that can do without ripper is not broken by the decline.
    let rescued = run_ruby(
        r#"
        begin
          require "ripper"
          puts "loaded"
        rescue LoadError
          puts "no ripper"
        end
        "#,
    );
    assert!(rescued.status.success(), "stderr: {}", rescued.stderr);
    assert_eq!(rescued.stdout, "no ripper\n");

    // The alternative the message points at is real and covered by the gem
    // probe (`require "prism"` compiles as a probed gem; its whole-graph
    // golden was retired from the suite). Not re-asserted here: it pulls the
    // gem, which cost this test minutes.
}

#[test]
fn requires_that_cannot_be_resolved_at_compile_time() {
    // A NON-LITERAL require target can't be resolved at compile time, so it
    // lowers to a runtime `Kernel#require` raising LoadError (matching CRuby),
    // rather than failing the compile.
    let result = run_ruby("name = \"nope_xyz\"\nrequire name\n");
    assert!(
        !result.status.success() && result.stderr.contains("cannot load such file -- nope_xyz"),
        "stderr: {}",
        result.stderr
    );

    // A LITERAL plain `require` of a feature that can't be found is NOT a
    // compile error: it lowers to a runtime `Kernel#require` raising LoadError,
    // exactly like the non-literal case above -- so it COMPILES, and the
    // optional-dependency idiom `begin; require "x"; rescue LoadError` catches
    // at runtime. (A RESOLVABLE feature is spliced even off top level; see the
    // positive-path tests below.)
    for src in [
        "def m\n  require \"x\"\nend\n",
        "begin\n  require \"optional_dep\"\nrescue LoadError\nend\n",
    ] {
        assert!(
            zeo::check_program(src).is_ok(),
            "expected {src:?} to compile (unresolvable require defers to runtime)"
        );
    }

    // A missing `require_relative`, by contrast, IS a compile error: it names a
    // project-local file that must exist, never an optional dependency.
    let err = zeo::check_program("require_relative \"no_such_sibling\"\n").unwrap_err();
    assert!(
        err.contains("cannot load such file") || err.contains("cannot infer basepath"),
        "unexpected error: {err}"
    );
}

// ---- gems: .gemspec manifests + search-path resolution ----
// Positive-path output oracle-verified by simulating package roots with
// real `ruby -I <pkg>/lib` (the package layer IS just ordered roots).

#[test]
fn packages_resolve_with_nested_features_and_cross_package_requires() {
    let result = run_ruby_packages(
        &[
            (
                "packages/greet/greet.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"greet\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "packages/greet/lib/greet.rb",
                r##"
                    require "greet/upper"
                    require "farewell"
                    module Greet
                      def self.hi(name)
                        Upper.dashed("hi, #{name}")
                      end
                    end
                    puts "greet loaded"
                "##,
            ),
            (
                "packages/greet/lib/greet/upper.rb",
                r##"
                    module Upper
                      def self.dashed(s)
                        "-- #{s} --"
                      end
                    end
                    puts "upper loaded"
                "##,
            ),
            (
                "packages/farewell/farewell.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"farewell\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "packages/farewell/lib/farewell.rb",
                r#"
                    module Farewell
                      def self.bye
                        "bye!"
                      end
                    end
                    puts "farewell loaded"
                "#,
            ),
            (
                "main.rb",
                r#"
                    require "greet"
                    puts Greet.hi("ann")
                    puts Farewell.bye
                "#,
            ),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "upper loaded\nfarewell loaded\ngreet loaded\n-- hi, ann --\nbye!\n"
    );
}

#[test]
fn a_caller_supplied_copy_of_a_substituted_gem_does_not_shadow_zeos_own() {
    // First-name-wins is the rule for ordinary packages (the test below), and
    // a caller dir is searched ahead of zeo's bundled tier on purpose. The
    // curated substitution set is the exception: zeo's native half IS `ffi`,
    // so the upstream gem's Ruby half is dead code -- it opens by requiring a
    // C extension zeo does not have, and every class it then defines is one
    // the native half already owns. Splicing it compiled a program that died
    // at load on `uninitialized constant FFI::TypeDefs`, while its computed
    // `require RUBY_VERSION... + "/ffi_c"` demanded the gem's whole load path
    // as units and dragged `ffi/struct_layout.rb` into the compile.
    let result = run_ruby_packages(
        &[
            (
                "packages/ffi/ffi.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"ffi\"\n  s.version = \"9.9.9\"\nend\n",
            ),
            (
                "packages/ffi/lib/ffi.rb",
                "module FFI\n  IMPOSTOR = true\nend\n",
            ),
            (
                "main.rb",
                r#"
                    require "ffi"
                    p defined?(FFI::IMPOSTOR)
                    p FFI::Platform.mac? == RUBY_PLATFORM.include?("darwin")
                "#,
            ),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\ntrue\n");
}

#[test]
fn a_feature_provided_by_two_packages_resolves_to_the_first_and_warns() {
    // Real Ruby never errors on a squatted feature name: RubyGems'
    // `find_by_path` answers the name-ascending first provider (alpha here),
    // Bundler the first activated. zeo follows, and says so once at the
    // require site -- the warning prints at program startup, where ruby's
    // own parse warnings print.
    let result = run_ruby_packages(
        &[
            (
                "packages/alpha/alpha.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"alpha\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            ("packages/alpha/lib/common.rb", "puts 1\n"),
            (
                "packages/beta/beta.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"beta\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            ("packages/beta/lib/common.rb", "puts 2\n"),
            ("main.rb", "require \"common\"\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
    assert!(
        result.stderr.contains("provided by multiple gems")
            && result.stderr.contains("alpha 1.0.0")
            && result.stderr.contains("beta 1.0.0")
            && result.stderr.contains("resolved to alpha"),
        "unexpected stderr: {}",
        result.stderr
    );
}

#[test]
fn the_root_gem_outranks_an_alphabetically_earlier_provider() {
    // `CompileOptions::root_gem` (the CLI's `--root-gem`, the gem probe's
    // subject): Bundler-root semantics. Without it, `alpha` would win the
    // squatted feature by RubyGems' name-ascending order; the root hint puts
    // `zeta` first. Asserted on the generated Rust's string literals -- the
    // spliced file's own text is the provider's identity.
    let dir = std::env::temp_dir().join("zeo-test-root-gem");
    for (rel, source) in [
        (
            "packages/alpha/alpha.gemspec",
            "Gem::Specification.new do |s|\n  s.name = \"alpha\"\n  s.version = \"1.0.0\"\nend\n",
        ),
        ("packages/alpha/lib/common.rb", "puts \"from-alpha\"\n"),
        (
            "packages/zeta/zeta.gemspec",
            "Gem::Specification.new do |s|\n  s.name = \"zeta\"\n  s.version = \"1.0.0\"\nend\n",
        ),
        ("packages/zeta/lib/common.rb", "puts \"from-zeta\"\n"),
        ("main.rb", "require \"common\"\n"),
    ] {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
    }
    let opts = zeo::CompileOptions {
        input_path: Some(dir.join("main.rb")),
        package_dirs: vec![dir.join("packages")],
        root_gem: Some(zeo::Gem::named("zeta")),
        ..Default::default()
    };
    let compiled = zeo::analyze_program("require \"common\"\n", &opts)
        .unwrap_or_else(|e| panic!("compile failed: {}", String::from(e)));
    // Ask the LOADER what it spliced rather than searching the emitted text
    // for a puts string: the question is which copy of `common.rb` won, and
    // `loaded_files` answers it exactly.
    let loaded: Vec<_> = compiled
        .compiler
        .hir
        .loader
        .loaded_files
        .iter()
        .map(|f| f.canonical.to_string_lossy().into_owned())
        .collect();
    assert!(
        loaded.iter().any(|p| p.contains("zeta")) && !loaded.iter().any(|p| p.contains("alpha")),
        "the root gem's copy should have been spliced, loaded: {loaded:?}"
    );
}

#[test]
fn strict_mode_restores_the_ambiguity_error() {
    // `ZEO_DEBUG=strict-ambiguous-require` keeps the old hard error for
    // callers who want squatting surfaced loudly. Safe to set here: nextest
    // runs each test in its own process.
    unsafe { std::env::set_var("ZEO_DEBUG", "strict-ambiguous-require") };
    let err = compile_packages(
        &[
            ("packages/alpha/alpha.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"alpha\"\n  s.version = \"1.0.0\"\nend\n"),
            ("packages/alpha/lib/common.rb", "puts 1\n"),
            ("packages/beta/beta.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"beta\"\n  s.version = \"1.0.0\"\nend\n"),
            ("packages/beta/lib/common.rb", "puts 2\n"),
            ("main.rb", "require \"common\"\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(
        err.contains("found in multiple gems") && err.contains("alpha") && err.contains("beta"),
        "unexpected error: {err}"
    );
}

// ---- base64 native-Rust package ----

#[test]
fn base64_package_matches_real_ruby() {
    // Oracle: real ruby's own bundled base64 gem (all expectations below
    // are its literal outputs, incl. encode64's 60-char line wrapping).
    let result = run_ruby_packages(
        &[(
            "main.rb",
            r#"
                require "base64"
                puts Base64.encode64("hello world")
                puts Base64.encode64("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                puts Base64.strict_encode64("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                puts Base64.decode64(Base64.encode64("round trip"))
                puts Base64.urlsafe_encode64("ab")
                puts Base64.urlsafe_decode64("YWI")
                puts Base64.strict_decode64("aGVsbG8=")
            "#,
        )],
        "main.rb",
        &[],
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "aGVsbG8gd29ybGQ=\n\
         YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFh\nYWFhYWE=\n\
         YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=\n\
         round trip\n\
         YWI=\n\
         ab\n\
         hello\n"
    );
}

// (Removed: `native_crate_is_linked_only_when_its_package_is_required` and
// `native_func_arity_is_checked_at_compile_time` -- both asserted the retired
// native-DSL linking behavior for `base64`, which is now an in-tree `ext/`
// module. `base64_package_matches_real_ruby` above now validates the in-tree
// path, and arity is enforced at runtime via `arity!` -> ArgumentError.)

// ---- The set pure-Ruby package + the dispatch fixes it forced ----

#[test]
fn set_package_matches_real_rubys_core_set() {
    // The flagship acceptance: every expectation below is real ruby's core
    // Set's literal output for the same program (oracle-verified) --
    // exercising include+MRO through a package, &blk forwarding through
    // nested escaping Procs, operator-method definitions, and the dynamic
    // builtin dispatch layer end to end.
    let result = run_ruby_packages(
        &[(
            "main.rb",
            r#"
                require "set"
                s = Set.new([1, 2, 3, 2, 1])
                puts s.size
                puts s.include?(2)
                s.add(4)
                s << 5
                s.delete(3)
                puts s.to_a.length
                puts s.member?(3)
                a = Set.new([1, 2, 3])
                b = Set.new([3, 4])
                puts (a | b).size
                puts (a & b).to_a.length
                puts (a - b).size
                puts (a ^ b).size
                puts a.subset?(Set.new([1, 2, 3, 9]))
                puts Set.new([3, 2, 1]) == Set.new([1, 2, 3])
                puts a == b
                doubled = a.map { |x| x * 2 }
                puts doubled.length
                puts a.select { |x| x > 1 }.length
                puts a.any? { |x| x > 2 }
                puts a.all? { |x| x > 0 }
                puts a.count
                puts Set.new.empty?
                copy = Set.new(a)
                puts copy.size
            "#,
        )],
        "main.rb",
        &[],
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\ntrue\n4\nfalse\n4\n1\n2\n3\ntrue\ntrue\nfalse\n3\n2\ntrue\ntrue\n3\ntrue\n3\n"
    );
}

// -- Ruby::Box. Oracle: `RUBY_BOX=1 ruby -W:no-experimental`, which is
// what `run_ruby_boxed`/`run_ruby_project_boxed` run the program under.
// Where our AOT model deliberately diverges -- the compile-time
// rejections -- the expectation states OUR documented behaviour.

/// The clean rejections: an expression-position `Ruby::Box.new` (a box
/// nothing could reference) and box operations outside their recognized
/// positions. (`.current`/`.enabled?` are ordinary runtime calls now that
/// the class carries real rows; a non-literal `box.eval` source is NOT
/// rejected here -- it routes to the runtime `eval`, so a non-string
/// source is a catchable runtime `TypeError`, exactly like `Kernel#eval`;
/// see `box_eval_dynamic_source_routes_through_the_vm`. Two shapes that
/// used to be rejected here compile now: an expression-position literal
/// `box.eval` defining a class (the registration walk descends every
/// container), and an expression-position `box.require` (which reaches the
/// box's own run-time load path).)
#[test]
fn ruby_box_rejections_are_clean_errors() {
    // An expression-position `.new` compiles to a dynamic send and raises
    // CRuby's own disabled-mode refusal at run time.
    let result =
        run_ruby("begin\n  p [Ruby::Box.new]\nrescue RuntimeError => e\n  puts e.message\nend\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Ruby Box is disabled. Set RUBY_BOX=1 environment variable to use Ruby::Box.\n"
    );
    // An expression-position `box.require` used to be a compile-time
    // rejection. It compiles now -- the ordinary send reaches the box's own
    // run-time load path -- and a target that resolves to nothing is the
    // `LoadError` it would be in ruby.
    zeo::check_program("box = Ruby::Box.new\nx = [box.require(\"f\")]\n")
        .expect("an expression-position box.require reaches the run-time load path now");
    // The once-rejected expression-position class-defining eval splice.
    zeo::check_program("box = Ruby::Box.new\nv = box.eval(\"class X; end\")\n")
        .expect("a literal box.eval defining a class registers and compiles now");
}

// --- G0 FAIL_RUSTC sweep: Object-boxing + emission fixes ---------------
//
// Each of these reproduces a generated-Rust compile failure (FAIL_RUSTC)
// found by the conformance corpus: zeo accepted the program but
// emitted ill-typed Rust.

// ---- gems with two halves: a native half plus a Ruby half ----

// `require "rbconfig"` resolves zeo's built-in synthetic shim (real Ruby
// generates rbconfig at build time; zeo ships a static stand-in), so
// RbConfig::CONFIG answers the version/platform/layout keys rubygems and
// bundler read.

// A `require` need not be a top-level statement. Loading the file still runs
// one written in a conditional, a `begin`, or a class body, so whole-program
// AOT splices it at compile time. A method body is the exception -- see
// `a_library_only_a_method_body_requires_is_not_compiled_in`. Oracle-pinned.

// A library ONLY a method body requires compiles in as a LAZY UNIT: the call
// stays a runtime `Kernel#require` and loads the unit at first execution,
// which is CRuby's order exactly. (It used to be omitted outright -- a
// deliberate divergence surfacing as LoadError -- because EAGER loading both
// reordered the program and dragged every lazy dependency into the binary;
// units keep the size win AND the semantics: `require "rubygems"` stays far
// below the 2.69M generated lines eager bundler-dragging produced.)

// A dynamic `load`/`require` (a runtime-computed target) does not fail the
// COMPILE -- whole-program AOT can't splice a path it only learns at runtime,
// so the call is lowered to a runtime `Kernel#{load,require}` that raises
// CRuby's `LoadError` if and when it actually executes. A guarded dynamic load
// (the `load ENV["X"] if ENV["X"]` idiom rubygems uses) compiles and no-ops
// when its guard is false.

#[test]
fn a_declined_units_class_bodies_are_declined_with_it() {
    // A gem that computes an autoload target has its whole load path compiled
    // in as units, and a unit that hits a gap is DECLINED: requiring it raises
    // LoadError naming the gap. Its class-body SITES have to go with it.
    //
    // They are hoisted by CLASS, not by statement stream, so a leftover site
    // made codegen emit the body of the `module Alpha` this unit opened -- and
    // that body holds the nested `class Leaf` the very same failure had
    // already stopped from registering. The compile then died naming `Leaf`,
    // "in a position the analyze walk doesn't register", pointing at the
    // leftover instead of the refusal that caused it and burying the real
    // reason deep in a require graph.
    let files = [
        (
            "packages/alpha/alpha.gemspec",
            "Gem::Specification.new do |s|\n  s.name = \"alpha\"\n  s.version = \"1.0.0\"\nend\n",
        ),
        (
            "packages/alpha/lib/alpha.rb",
            // Computed target: the pre-pass cannot name it, so the load path
            // is compiled in as units and `alpha/leaf` becomes one.
            "module Alpha\n  autoload :Leaf, \"alpha/lea\" + \"f\"\nend\n",
        ),
        (
            "packages/alpha/lib/alpha/leaf.rb",
            // Any gap inside the nested class does; subclassing a built-in
            // with no generated struct is simply the shortest one to write.
            "module Alpha\n  class Leaf < Binding\n    def hi = \"hi\"\n  end\nend\n",
        ),
        ("main.rb", "require \"alpha\"\nputs \"compiled\"\n"),
    ];
    let result = run_ruby_packages(&files, "main.rb", &[], &["packages"]);
    assert!(
        result.status.success(),
        "the decline must stay a decline, not a compile failure; stderr: {}",
        result.stderr
    );
    assert_eq!(result.stdout, "compiled\n");
}

#[test]
fn an_absent_autoload_target_is_left_to_the_constant_read() {
    // An eager autoload splice is not a require: ruby does not touch the file
    // until the constant is read. A LITERAL target zeo could name was spliced
    // regardless, so a target that is simply not on the load path failed the
    // whole compile -- actionpack's `autoload :Test, "rack/test"` took every
    // gem that reaches action_dispatch without rack-test alongside it, 235
    // ledger rows.
    //
    // The gem still compiles when the target is absent, and the constant read
    // raises the LoadError ruby raises there. `alpha` also computes an
    // autoload target, so its whole load path is compiled in as units -- the
    // shape that used to raise at the DECLARATION instead.
    let files = [
        (
            "packages/alpha/alpha.gemspec",
            "Gem::Specification.new do |s|\n  s.name = \"alpha\"\n  s.version = \"1.0.0\"\nend\n",
        ),
        (
            "packages/alpha/lib/alpha.rb",
            "module Alpha\n  autoload :Leaf, \"alpha/lea\" + \"f\"\n  \
             autoload :Absent, \"definitely_no_such_feature_xyz\"\nend\n",
        ),
        (
            "packages/alpha/lib/alpha/leaf.rb",
            "module Alpha\n  class Leaf\n    def hi = \"hi\"\n  end\nend\n",
        ),
        (
            "main.rb",
            "require \"alpha\"\n\
             puts \"loaded\"\n\
             p Alpha::Leaf.new.hi\n\
             p Alpha.autoload?(:Absent)\n\
             begin\n  Alpha::Absent\nrescue LoadError => e\n  \
             puts \"LoadError: #{e.message}\"\nend\n\
             puts \"done\"\n",
        ),
    ];
    let result = run_ruby_packages(&files, "main.rb", &[], &["packages"]);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "loaded\n\"hi\"\n\"definitely_no_such_feature_xyz\"\n\
         LoadError: cannot load such file -- definitely_no_such_feature_xyz\ndone\n"
    );
}

/// `--strict-static-require` refuses a target this compile cannot resolve,
/// and only that: a literal require still compiles.
#[test]
fn strict_static_require_refuses_a_computed_target() {
    let err = compile_project_strict(
        &[("prog.rb", "require [\"set\", \"\"].first\n")],
        "prog.rb",
        &[],
    )
    .expect_err("a computed require is refused under --strict-static-require");
    assert!(
        err.contains("--strict-static-require"),
        "unexpected error: {err}"
    );
    compile_project_strict(
        &[("prog.rb", "require \"set\"\np Set[1].size\n")],
        "prog.rb",
        &[],
    )
    .expect("a literal require resolves at compile time");
}

#[test]
fn a_swept_units_definitions_wait_for_the_unit_to_run() {
    // A computed `require` anywhere in a package sweeps every `.rb` in it
    // into feature units, so a run-time `require <var>` can reach any of
    // them. Their `def`s register at startup all the same -- the static MRO
    // needs a shape -- and used to ANSWER from startup, inventing methods on
    // Object, on a reopened builtin, on a user class, on a module and on the
    // class-method channel. ruby has none of them until the file runs.
    //
    // Oracle-verified against ruby 4.0.6 (`ruby -Ipackages/leaky/lib`), both
    // halves: every probe raises before, and every probe answers after.
    let files = &[
        (
            "packages/leaky/leaky.gemspec",
            "Gem::Specification.new do |s|\n  s.name = \"leaky\"\n  s.version = \"1.0.0\"\nend\n",
        ),
        (
            "packages/leaky/lib/leaky.rb",
            r#"
                module Leaky
                  def self.load_one(name)
                    require name
                  end
                end
            "#,
        ),
        (
            "packages/leaky/lib/leaky/script.rb",
            r#"
                def leaked_helper(a, b) = [a, b]
                class String; def leaked_on_string = :from_unit; end
                class MainClass; def leaked_on_main_class = :from_unit; end
                module MainMod; def leaked_on_main_mod = :from_unit; end
                class MainClass; def self.leaked_class_method = :from_unit; end
                LEAKED_CONST = :from_unit
            "#,
        ),
    ];
    let probes = r#"
        def t(label)
          print label, ": "
          begin
            p(yield)
          rescue => e
            p e.class
          end
        end
        def probe_all
          t("toplevel") { leaked_helper(1, 2) }
          t("builtin") { "x".leaked_on_string }
          t("user class") { MainClass.new.leaked_on_main_class }
          t("module") { Host.new.leaked_on_main_mod }
          t("class method") { MainClass.leaked_class_method }
          t("constant") { LEAKED_CONST }
          t("respond_to?") { MainClass.new.respond_to?(:leaked_on_main_class) }
          t("instance_methods") { MainClass.instance_methods(false) }
          t("module methods") { MainMod.instance_methods(false) }
        end
    "#;

    let preamble = "class MainClass; end\nmodule MainMod; end\nclass Host; include MainMod; end\nrequire \"leaky\"\n";
    let before_src = format!("{preamble}{probes}\nprobe_all\n");
    let mut before = files.to_vec();
    before.push(("main.rb", &before_src));
    let result = run_ruby_packages(&before, "main.rb", &[], &["packages"]);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "toplevel: NoMethodError\n\
         builtin: NoMethodError\n\
         user class: NoMethodError\n\
         module: NoMethodError\n\
         class method: NoMethodError\n\
         constant: NameError\n\
         respond_to?: false\n\
         instance_methods: []\n\
         module methods: []\n"
    );

    // ...and the other half: requiring the file makes every one of them real.
    let after_src = format!("{preamble}{probes}\nLeaky.load_one(\"leaky/script\")\nprobe_all\n");
    let mut after = files.to_vec();
    after.push(("main.rb", &after_src));
    let result = run_ruby_packages(&after, "main.rb", &[], &["packages"]);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "toplevel: [1, 2]\n\
         builtin: :from_unit\n\
         user class: :from_unit\n\
         module: :from_unit\n\
         class method: :from_unit\n\
         constant: :from_unit\n\
         respond_to?: true\n\
         instance_methods: [:leaked_on_main_class]\n\
         module methods: [:leaked_on_main_mod]\n"
    );
}
