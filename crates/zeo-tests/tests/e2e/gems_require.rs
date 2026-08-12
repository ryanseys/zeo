use crate::support::{
    compile_packages, compile_project, run_ruby, run_ruby_packages, run_ruby_project,
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
    zeo::compile_to_rust_with(
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
    // The classification behind `cargo xtask gem-compat`, over the
    // self-contained fixture store -- pure Ruby compiles, a native-extension
    // gem and a precompiled-only gem are both native-unsupported.
    use zeo::GemCompatOutcome;
    let store =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gem_store/store");
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
    assert!(matches!(
        outcome("nativelib"),
        GemCompatOutcome::NativeUnsupported { ref kind, .. } if kind == "native-extension"
    ));
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
        Some(GemCompatOutcome::NativeUnsupported { .. })
    ));
    assert!(matches!(
        sweep_outcome("precompiled"),
        Some(GemCompatOutcome::NativeUnsupported { .. })
    ));
}

#[test]
fn match_required_one_liner_binds_on_success() {
    let result = run_ruby(
        r#"
        arr = [1, 2]
        arr => [first, second]
        puts first
        puts second
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n");
}

#[test]
fn match_required_one_liner_raises_no_matching_pattern_error_on_failure() {
    let result = run_ruby("5 => String\n");
    assert!(!result.status.success());
    assert!(
        result
            .stderr
            .contains("in '<main>': 5 (NoMatchingPatternKeyError)")
            || result.stderr.contains("NoMatchingPattern"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn object_typed_local_widened_to_poly_across_branches_boxes_correctly() {
    // `x`'s two branches assign DIFFERENT classes, so
    // `analyze::locals::merge_locals` widens its whole-scope type to `Poly`
    // (a plain `RubyValue` slot) -- but each branch's own `HirNode::New`
    // codegen still produces a bare, unboxed `Arc<Concrete>` unless the
    // write site itself boxes it. Before this fix, this was
    // a genuine `rustc` type-mismatch in the GENERATED Rust, not just a
    // wrong answer.
    let result = run_ruby(
        r#"
        class Foo
          def initialize
            @tag = "foo"
          end
          def tag
            @tag
          end
        end

        class Bar
          def initialize
            @tag = "bar"
          end
          def tag
            @tag
          end
        end

        class Picker
          def pick(flag)
            if flag
              x = Foo.new
            else
              x = Bar.new
            end
            x
          end
        end

        p = Picker.new
        puts p.pick(true).tag
        puts p.pick(false).tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "foo\nbar\n");
}

// ---- require/require_relative/load compile-time splicing ----
// Every positive-path expectation below was oracle-verified against real
// `ruby` (4.0.6) first, per this project's standing convention.

#[test]
fn require_relative_splices_in_document_order_and_shares_the_global_namespace() {
    let result = run_ruby_project(
        &[
            // Top-level `def` in ANY file (main or required) is a separate,
            // pre-existing gap ("unexpected top-level-only node"), routed
            // around with classes/module functions here exactly like every
            // prior phase's tests.
            (
                "greeter.rb",
                r##"
                    GREETING = "hello"
                    class Greeter
                      def greet(name)
                        "#{GREETING}, #{name}!"
                      end
                    end
                    puts "greeter loaded"
                "##,
            ),
            (
                "main.rb",
                r#"
                    puts "before require"
                    require_relative "greeter"
                    puts "after require"
                    g = Greeter.new
                    puts g.greet("world")
                    puts GREETING
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "before require\ngreeter loaded\nafter require\nhello, world!\nhello\n"
    );
}

#[test]
fn circular_requires_compose_in_rubys_execution_order() {
    // CRuby registers a feature as loading BEFORE executing it, so the
    // inner require of an in-progress file is a no-op and execution order
    // is ca-start, all of cb, ca-end -- the dedup-before-lowering rule
    // reproduces this exactly.
    let result = run_ruby_project(
        &[
            (
                "ca.rb",
                "puts \"ca start\"\nrequire_relative \"cb\"\nputs \"ca end\"\n",
            ),
            (
                "cb.rb",
                "puts \"cb start\"\nrequire_relative \"ca\"\nputs \"cb end\"\n",
            ),
            ("main.rb", "require_relative \"ca\"\nputs \"main done\"\n"),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ca start\ncb start\ncb end\nca end\nmain done\n"
    );
}

#[test]
fn plain_require_resolves_against_search_roots_including_nested_features() {
    let result = run_ruby_project(
        &[
            (
                "vendorlib/util.rb",
                r#"
                    require "util/strings"
                    UTIL = "util root"
                    puts "util loaded"
                "#,
            ),
            (
                "vendorlib/util/strings.rb",
                r#"
                    module Spacer
                      def self.doubled(n)
                        n * 2
                      end
                    end
                    puts "util/strings loaded"
                "#,
            ),
            (
                "main.rb",
                r#"
                    require "util"
                    puts UTIL
                    puts Spacer.doubled(21)
                "#,
            ),
        ],
        "main.rb",
        &["vendorlib"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "util/strings loaded\nutil loaded\nutil root\n42\n"
    );
}

#[test]
fn autoload_with_a_dynamic_feature_defers_to_the_runtime_row() {
    // A target the compile-time splice cannot name is no longer a rejection:
    // it lowers to a real `Module#autoload` call, which resolves the string the
    // program actually builds against the load path compiled in for it
    // (`zeo_rt::features`). Compiling is the assertion -- what the call then
    // finds is covered by `tests/autoload_dsl_computes_its_own_path.rb`.
    assert!(
        zeo::compile_to_rust("autoload :X, some_method_call").is_ok(),
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
fn a_require_under_a_statically_false_engine_guard_is_pruned_not_spliced() {
    // `require X if RUBY_ENGINE == "jruby"` names another engine's code; a
    // whole-program AOT compiler eager-splices every literal require it sees,
    // but this one's guard folds statically false on CRuby-targeting zeo, so
    // the target must be PRUNED, not spliced (splicing would fail to resolve
    // it). The program compiles and the guarded require is simply dead.
    let result =
        run_ruby("require \"no_such_engine_lib_xyz\" if RUBY_ENGINE == \"jruby\"\nputs \"ok\"\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok\n");
}

#[test]
fn an_unresolvable_require_inside_rescue_loaderror_defers_to_runtime() {
    // The optional-dependency idiom: `begin; require 'missing'; rescue
    // LoadError`. A literal require that can't be resolved but is lexically
    // inside a `rescue LoadError` is deferred to a runtime `Kernel#require`
    // (raising the LoadError the rescue handles), not a loud compile error.
    // net/http's `begin; require 'win32/sspi'; rescue LoadError` needs this.
    let result = run_ruby(
        r#"
        begin
          require "definitely_missing_optdep_xyz"
          puts "loaded"
        rescue LoadError => e
          puts "rescued: #{e.send(:message)}"
        end
        puts "after"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "rescued: cannot load such file -- definitely_missing_optdep_xyz\nafter\n"
    );
}

#[test]
fn open3_lowers_from_the_vendored_gem_with_its_jruby_require_pruned() {
    // open3's last line is `require 'open3/jruby_windows' if RUBY_ENGINE ==
    // 'jruby' && ...`; that file is JRuby-native (`require 'jruby'`, java_import)
    // and must be pruned rather than spliced. Loading open3 and reflecting on
    // its API needs no subprocess, so this exercises the load path end-to-end.
    let result = run_ruby(
        "require \"open3\"\nputs Open3.respond_to?(:capture3)\nputs Open3.respond_to?(:popen3)\n",
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\n");
}

#[test]
fn shellwords_class_method_aliases_run_from_the_vendored_gem() {
    // shellwords defines its module methods (`module_function`) then aliases
    // them inside `class << self` (`alias split shellsplit`) -- a class-method
    // alias that resolves against the singleton table, not instance methods.
    let result = run_ruby(
        r#"
        require "shellwords"
        p Shellwords.split('a "b c"')
        puts Shellwords.escape("a b")
        puts Shellwords.join(["a", "b c"])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"a\", \"b c\"]\na\\ b\na b\\ c\n");
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
    // gem and the eval-vm runtime, which cost this test minutes.
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
            zeo::compile_to_rust(src).is_ok(),
            "expected {src:?} to compile (unresolvable require defers to runtime)"
        );
    }

    // A missing `require_relative`, by contrast, IS a compile error: it names a
    // project-local file that must exist, never an optional dependency.
    let err = zeo::compile_to_rust("require_relative \"no_such_sibling\"\n").unwrap_err();
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
fn a_feature_provided_by_two_packages_resolves_to_the_first_and_warns() {
    // Real Ruby never errors on a squatted feature name: RubyGems'
    // `find_by_path` answers the name-ascending first provider (alpha here),
    // Bundler the first activated. zeo follows, and says so once at the
    // require site -- the warning prints at program startup, where ruby's
    // own parse warnings print.
    let result = run_ruby_packages(
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
    let compiled = zeo::compile_to_rust_with("require \"common\"\n", &opts)
        .unwrap_or_else(|e| panic!("compile failed: {}", String::from(e)));
    assert!(
        compiled.rust_source.contains("from-zeta")
            && !compiled.rust_source.contains("from-alpha"),
        "the root gem's copy should have been spliced"
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
        &[crate::REPO_PACKAGES],
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

#[test]
fn require_gated_extension_constant_is_a_name_error_without_its_require() {
    // The ext require-gate: a require-gated builtin's constant
    // (`Base64`, gated by `"base64"`) is INVISIBLE until `require "base64"`
    // activates it -- referencing it un-required raises `NameError:
    // uninitialized constant Base64`, oracle-verified against ruby 4.0.6
    // (`uninitialized constant Base64 (NameError)`). The gate rides the
    // ordinary `resolve_class` -> unset-constant path, so it's a rescuable
    // RUNTIME NameError, not a compile error.
    let result = run_ruby(r#"puts Base64.strict_encode64("hi")"#);
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uninitialized constant Base64"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn require_gated_extension_name_error_is_rescuable() {
    // Same gate, caught: since the miss surfaces through the runtime
    // constant path it's an ordinary rescuable `NameError` (oracle prints
    // `rescued: uninitialized constant Base64`). This program NEVER requires
    // base64, so activation stays off -- avoiding the documented
    // program-global-activation divergence (a `require` anywhere activates
    // the constant everywhere, unlike CRuby's file-ordered visibility).
    let result = run_ruby(
        r#"
        begin
          Base64.strict_encode64("hi")
        rescue NameError => e
          puts "rescued: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: uninitialized constant Base64\n");
}

#[test]
fn scaffolded_extension_constant_resolves_but_is_a_name_error_without_require() {
    // A scaffolded ext (json) is still require-gated: its constant is a
    // NameError until `require "json"` fires, exactly like the fully-built
    // ones -- scaffolding changes only what the METHODS do, not the gate.
    let result = run_ruby(r#"puts JSON.generate([1])"#);
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uninitialized constant JSON"),
        "stderr: {}",
        result.stderr
    );
}

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
        &[crate::REPO_PACKAGES],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\ntrue\n4\nfalse\n4\n1\n2\n3\ntrue\ntrue\nfalse\n3\n2\ntrue\ntrue\n3\ntrue\n3\n"
    );
}

#[test]
fn object_new_builds_a_distinct_boxed_sentinel() {
    // `Object.new` emitted `Arc::new(zeo_rt::Object)` -- using the struct
    // `Object` (a private-field struct, not a unit struct) as a value, an
    // E0423 that never compiled. It must be `Object::default()`. Each call is
    // a fresh Arc, so two sentinels are distinct by identity.
    let result = run_ruby(
        r#"
        A = Object.new
        B = Object.new
        p A == B
        p A == A
        p A.is_a?(Object)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\ntrue\ntrue\n");
}

// -- Ruby::Box. Oracle: `RUBY_BOX=1 ruby -W:no-experimental`
// (where our AOT model deliberately diverges -- handle inspect suffix,
// compile-time rejections -- the expectation below states OUR documented
// behavior).

/// The keystone: a box-required file's classes are DISTINCT from main's
/// (same file, different class objects), builtins are SHARED
/// (`box::String == String`), a box top-level constant is readable
/// externally, and box gvars are invisible in main -- all per the CRuby
/// box model.
#[test]
fn box_isolation_and_shared_builtins() {
    let result = run_ruby_project(
        &[
            (
                "widget.rb",
                "class Widget\n  def hi\n    \"box widget\"\n  end\nend\nWIDGET_CONST = 99\n$box_g = 5\n",
            ),
            (
                "main.rb",
                "class Widget\n  def hi\n    \"main widget\"\n  end\nend\n\
                 box = Ruby::Box.new\n\
                 box.require_relative \"widget\"\n\
                 p Widget.new.hi\n\
                 w = box::Widget.new\n\
                 p w.hi\n\
                 p box::Widget == Widget\n\
                 p box::String == String\n\
                 p box::WIDGET_CONST\n\
                 p $box_g\n\
                 p box\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"main widget\"\n\"box widget\"\nfalse\ntrue\n99\nnil\n#<Ruby::Box:1>\n"
    );
}

/// `Box#eval`: a statement-position eval may DEFINE classes in the box; an
/// expression-position eval returns a value whose static type is the box's
/// own class (Path 1 dispatch on a cross-boundary instance); two boxes
/// eval'ing the same class name get distinct classes.
#[test]
fn box_eval_defines_and_returns_across_the_boundary() {
    let result = run_ruby(
        r#"
        box = Ruby::Box.new
        box.eval("class Gadget; def spin; 'spinning'; end; end")
        g = box.eval("Gadget.new")
        p g.spin
        p box::Gadget.new.spin
        box2 = Ruby::Box.new
        box2.eval("class Gadget; def spin; 'other'; end; end")
        p box2::Gadget.new.spin
        p(box::Gadget == box2::Gadget)
        p box.eval("1 + 2")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"spinning\"\n\"spinning\"\n\"other\"\nfalse\n3\n"
    );
}

/// A NON-literal `box.eval` source (a variable, a computed string) routes
/// through the runtime eval VM in the box's dimension -- the same fall-through
/// `Kernel#eval` uses for a dynamic source, but carrying `box_id`. A dynamic
/// eval reads/calls the box's own (`require`-defined) constants and classes,
/// its globals stay isolated from main, and a non-String source raises a
/// catchable `TypeError` at runtime rather than a compile error.
#[test]
fn box_eval_dynamic_source_routes_through_the_vm() {
    let result = run_ruby_project(
        &[
            (
                "lib.rb",
                "WIDGET_CONST = \"widget!\"\n\
                 class Widget\n  def self.describe = \"a widget\"\nend\n",
            ),
            (
                "main.rb",
                "box = Ruby::Box.new\n\
                 box.require_relative \"lib\"\n\
                 code = \"1 + 2\"\n\
                 p box.eval(code)\n\
                 p box.eval(\"WIDGET_CONST\")\n\
                 p box.eval(\"Widget.describe\")\n\
                 $g = \"main value\"\n\
                 read = \"$g\"\n\
                 p box.eval(read)\n\
                 box.eval(\"$g = 'box value'\")\n\
                 p $g\n\
                 p box.eval(read)\n\
                 begin\n  box.eval(123)\nrescue TypeError => e\n  puts \"rescued: #{e.message}\"\nend\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\n\"widget!\"\n\"a widget\"\nnil\n\"main value\"\n\"box value\"\n\
         rescued: no implicit conversion of Integer into String\n"
    );
}

/// Exceptions cross the boundary as plain references: a box-defined
/// `BoxError < StandardError` raised from box code is rescuable in main
/// through the SHARED bootstrap superclass chain, and `e.class` names it.
#[test]
fn box_exceptions_are_rescuable_in_main() {
    let result = run_ruby_project(
        &[
            (
                "thrower.rb",
                "class BoxError < StandardError\nend\n\
                 class Thrower\n  def self.go\n    raise BoxError, \"from the box\"\n  end\nend\n",
            ),
            (
                "main.rb",
                "box = Ruby::Box.new\n\
                 box.require_relative \"thrower\"\n\
                 begin\n  box::Thrower.go\nrescue StandardError => e\n  puts \"rescued: #{e.message} (#{e.class})\"\nend\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: from the box (BoxError)\n");
}

/// Gvar isolation is BIDIRECTIONAL (separate per-box tables, no fallback):
/// a box reads nil for main's `$g`, and a box's write never reaches main.
#[test]
fn box_globals_are_fully_separate() {
    let result = run_ruby(
        r#"
        $g = "main value"
        box = Ruby::Box.new
        p box.eval("$g")
        box.eval("$g = 'box value'")
        p $g
        p box.eval("$g")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\n\"main value\"\n\"box value\"\n");
}

/// The clean rejections: an expression-position `Ruby::Box.new` (a box
/// nothing could reference) and box operations outside their recognized
/// positions. (`.current`/`.enabled?` are ordinary runtime calls now that
/// the class carries real rows; a non-literal `box.eval` source is NOT
/// rejected here -- it routes to the runtime eval VM, so a non-string
/// source is a catchable runtime `TypeError`, exactly like `Kernel#eval`;
/// see `box_eval_dynamic_source_routes_through_the_vm`. An
/// expression-position literal `box.eval` defining a class used to be
/// rejected too -- the registration walk couldn't see into the splice --
/// but the walk descends every container now, so it registers and
/// compiles.)
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
    let err = zeo::compile_to_rust("box = Ruby::Box.new\nx = [box.require(\"f\")]\n").unwrap_err();
    assert!(err.contains("top-level statement"), "{err}");
    // The once-rejected expression-position class-defining eval splice.
    zeo::compile_to_rust("box = Ruby::Box.new\nv = box.eval(\"class X; end\")\n")
        .expect("a literal box.eval defining a class registers and compiles now");
}

/// A main-only class/constant is INVISIBLE inside a box (boxes dup from
/// MASTER, not main): resolving it inside box.eval raises NameError-shaped
/// failures rather than leaking main's definitions.
#[test]
fn main_definitions_are_invisible_inside_a_box() {
    let result = run_ruby(
        r#"
        MAIN_ONLY = 42
        box = Ruby::Box.new
        box.eval("begin; p MAIN_ONLY; rescue NameError => e; puts 'invisible'; end")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "invisible\n");
}

/// The other half of that line: a box IS a copy of master, so the constants
/// the runtime installed before main ran stay visible inside it -- the bare
/// read's fallback tail reaches those and stops, rather than continuing into
/// main's own `Object` table.
#[test]
fn master_constants_stay_visible_inside_a_box() {
    let result = run_ruby(
        r#"
        VERSION_COPY = RUBY_VERSION
        box = Ruby::Box.new
        box.eval("puts RUBY_VERSION")
        box.eval("begin; p VERSION_COPY; rescue NameError; puts 'invisible'; end")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        format!("{}\ninvisible\n", zeo_abi::RUBY_VERSION)
    );
}

#[test]
fn shift_operator_boxes_an_object_typed_argument() {
    // `junk << Trash.new(1)` on a Poly receiver: the numeric-op fallback's
    // match scrutinee must box the unboxed `Arc<Concrete>` argument
    // (found by the conformance corpus's argv_gc as a FAIL_RUSTC).
    let result = run_ruby(
        r#"
        class Trash
          def initialize(n)
            @n = n
          end
          attr_reader :n
        end
        def build
          junk = []
          junk << Trash.new(7)
          junk
        end
        puts build.length
        puts build[0].n
        "#,
    );
    assert_eq!(result.stdout, "1\n7\n");
    assert_eq!(result.stderr, "");
}

// --- G0 FAIL_RUSTC sweep: Object-boxing + emission fixes ---------------
//
// Each of these reproduces a generated-Rust compile failure (FAIL_RUSTC)
// found by the conformance corpus: zeo accepted the program but
// emitted ill-typed Rust.

#[test]
fn if_arms_with_different_object_classes_box_to_ruby_value() {
    let result = run_ruby(
        r#"
        class A
          def tag = "a"
        end
        class B
          def tag = "b"
        end
        pick = true
        x = pick ? A.new : B.new
        puts x.send(:tag)
        "#,
    );
    assert_eq!(result.stdout, "a\n");
}

#[test]
fn return_of_an_object_typed_value_boxes() {
    let result = run_ruby(
        r#"
        class Box
          def initialize(tag)
            @tag = tag
          end
          attr_reader :tag
        end
        def find_or_nil(want)
          if want == "yes"
            return Box.new("found")
          end
          nil
        end
        r = find_or_nil("yes")
        puts r.send(:tag)
        puts find_or_nil("no").inspect
        "#,
    );
    assert_eq!(result.stdout, "found\nnil\n");
}

#[test]
fn boolean_operators_box_object_typed_operands() {
    let result = run_ruby(
        r#"
        class Flag; end
        f = Flag.new
        puts (f && 1).inspect
        puts (nil || Flag.new).class
        puts f ? "truthy" : "falsy"
        puts (!f).inspect
        "#,
    );
    assert_eq!(result.stdout, "1\nFlag\ntruthy\nfalse\n");
}

#[test]
fn yield_boxes_an_object_typed_argument() {
    // (`Builder.new { }` can't be used here: `.new` doesn't forward its
    // block to `initialize` yet -- a separate, pre-existing gap.)
    let result = run_ruby(
        r#"
        class Builder
          def run
            yield self
          end
          def ping = "pong"
        end
        Builder.new.run { |b| puts b.send(:ping) }
        "#,
    );
    assert_eq!(result.stdout, "pong\n");
}

#[test]
fn bare_super_forwards_a_child_optional_into_a_parent_required_slot() {
    // The child's and parent's parameter SHAPES differ, so the forwarding
    // must flatten positionally rather than match bucket-for-bucket.
    let result = run_ruby(
        r##"
        class Parent
          def greet(name)
            "Parent(#{name})"
          end
        end
        class Child < Parent
          def greet(name = "default")
            super
          end
        end
        puts Child.new.greet
        puts Child.new.greet("given")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Parent(default)\nParent(given)\n");
}

/// A `require` of a natively-provided feature is a compile-time act, so it
/// works from any position -- and it has CRuby's real return value, true the
/// first time and false thereafter (`load.c:1413`).
#[test]
fn require_of_a_builtin_feature_works_from_any_position() {
    let result = run_ruby(
        r##"
        p(require "digest")
        p(require "digest")
        if 1 > 0
          require "json"
        end
        def load_it
          require "set"
          "ok"
        end
        p load_it
        require "base64" if false
        require "zlib" rescue nil
        puts "done"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\n\"ok\"\ndone\n");
}

/// `set` and `monitor` are already loaded before a CRuby program's first
/// line, so even their FIRST require answers false.
#[test]
fn require_of_a_boot_preloaded_feature_is_false_even_the_first_time() {
    let result = run_ruby("p(require \"set\")\np(require \"monitor\")\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\nfalse\n");
}

/// `Monitor` is invisible without its require -- the ext gate, not a
/// permanently-present constant.
#[test]
fn monitor_needs_its_require() {
    let result = run_ruby("Monitor.new\n");
    assert!(
        !result.status.success() || result.stderr.contains("Monitor"),
        "Monitor must not resolve without `require \"monitor\"`: {}",
        result.stderr
    );
}

// ---- gems with two halves: a native half plus a Ruby half ----

/// A gem's Ruby half can reopen its feature-gated NATIVE half and nest an
/// exception class inside it, which the native half then raises by name.
/// This is what retires the `StringScanner::Error` -> `RuntimeError`
/// divergence: a nested user exception registers under its fully qualified
/// name with a real constructor, where an ABI row cannot.
#[test]
fn a_gems_ruby_half_supplies_the_exception_class_its_native_half_raises() {
    let result = run_ruby(
        r##"
        require "json"
        require "strscan"
        begin
          JSON.parse("{oops")
        rescue JSON::ParserError => e
          p e.class.name
          p e.class.ancestors.include?(StandardError)
        end
        p JSON::ParserError.superclass.name
        p StringScanner::Error.ancestors.include?(StandardError)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"JSON::ParserError\"\ntrue\n\"JSON::JSONError\"\ntrue\n"
    );
}

// `require "rbconfig"` resolves zeo's built-in synthetic shim (real Ruby
// generates rbconfig at build time; zeo ships a static stand-in), so
// RbConfig::CONFIG answers the version/platform/layout keys rubygems and
// bundler read.
#[test]
fn rbconfig_shim_is_built_in() {
    let result = run_ruby(
        r#"
        require "rbconfig"
        puts RbConfig::CONFIG["ruby_version"]
        puts RbConfig::CONFIG.fetch("host_os")
        puts RbConfig::CONFIG["EXEEXT"].inspect
        puts RbConfig::CONFIG["arch"]
        puts defined?(RbConfig::CONFIG)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4.0.0\ndarwin25\n\"\"\narm64-darwin25\nconstant\n"
    );
}

// A `require` need not be a top-level statement. Loading the file still runs
// one written in a conditional, a `begin`, or a class body, so whole-program
// AOT splices it at compile time. A method body is the exception -- see
// `a_library_only_a_method_body_requires_is_not_compiled_in`. Oracle-pinned.
#[test]
fn require_works_in_non_top_level_positions() {
    let result = run_ruby(
        r#"
        require "ostruct" if RUBY_VERSION
        def make_struct
          require "ostruct"
          OpenStruct.new(a: 1, b: 2).b
        end
        puts make_struct

        begin
          require "set"
        rescue LoadError
          abort "no set"
        end
        puts Set.new([1, 1, 2, 3]).size

        def with_json
          begin
            require "json"
          rescue LoadError
            return "no json"
          end
          JSON.generate({ "k" => 1 })
        end
        puts with_json
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n3\n{\"k\":1}\n");
}

// A library ONLY a method body requires compiles in as a LAZY UNIT: the call
// stays a runtime `Kernel#require` and loads the unit at first execution,
// which is CRuby's order exactly. (It used to be omitted outright -- a
// deliberate divergence surfacing as LoadError -- because EAGER loading both
// reordered the program and dragged every lazy dependency into the binary;
// units keep the size win AND the semantics: `require "rubygems"` stays far
// below the 2.69M generated lines eager bundler-dragging produced.)
#[test]
fn a_library_only_a_method_body_requires_loads_lazily() {
    let result = run_ruby(
        r#"
        def lazy
          require "ostruct"
          o = OpenStruct.new(x: 1)
          o.x
        rescue LoadError => e
          e.message
        end
        puts lazy
        puts defined?(OpenStruct) ? "visible after load" : "invisible"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\nvisible after load\n");
}

// A dynamic `load`/`require` (a runtime-computed target) does not fail the
// COMPILE -- whole-program AOT can't splice a path it only learns at runtime,
// so the call is lowered to a runtime `Kernel#{load,require}` that raises
// CRuby's `LoadError` if and when it actually executes. A guarded dynamic load
// (the `load ENV["X"] if ENV["X"]` idiom rubygems uses) compiles and no-ops
// when its guard is false.
#[test]
fn dynamic_load_compiles_and_raises_loaderror_only_when_executed() {
    let result = run_ruby(
        r#"
        # Guarded dynamic load: guard false -> never executes -> no error.
        path = nil
        load path if path
        puts "guard_ok"

        # A dynamic load that DOES execute raises a rescuable LoadError.
        missing = "/no/such/file.rb"
        begin
          load missing
        rescue LoadError => e
          puts e.message
        end

        # A dynamic require behaves the same, and the program continues.
        name = "definitely_missing_lib_" + "xyz"
        begin
          require name
        rescue LoadError => e
          puts e.message
        end
        puts "done"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "guard_ok\ncannot load such file -- /no/such/file.rb\ncannot load such file -- definitely_missing_lib_xyz\ndone\n"
    );
}

#[test]
fn fileutils_core_commands_run_from_the_vendored_gem() {
    // `require "fileutils"` compiles the real vendored gem (gems/fileutils),
    // including its load-time module metaprogramming (module_function, extend
    // self, class << self, a platform-conditional StreamUtils_), and the core
    // file operations bundler relies on work.
    let result = run_ruby(
        r##"
        require "fileutils"
        puts FileUtils::VERSION
        puts FileUtils.respond_to?(:mkdir_p)
        d = "/tmp/zeo_fu_e2e_#{Process.pid}"
        FileUtils.rm_rf(d)
        FileUtils.mkdir_p("#{d}/a/b")
        puts Dir.exist?("#{d}/a/b")
        File.write("#{d}/a/f", "hi")
        FileUtils.cp("#{d}/a/f", "#{d}/a/g")
        puts File.read("#{d}/a/g")
        FileUtils.mv("#{d}/a/g", "#{d}/a/h")
        puts File.exist?("#{d}/a/h")
        FileUtils.rm_rf(d)
        puts Dir.exist?(d)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1.8.0\ntrue\ntrue\nhi\ntrue\nfalse\n");
}

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
