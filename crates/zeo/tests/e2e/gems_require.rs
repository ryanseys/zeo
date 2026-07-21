use crate::support::{run_ruby, run_ruby_project, run_ruby_packages, compile_project, compile_packages};

#[test]
fn gem_disclosure_report_records_how_each_library_was_satisfied() {
    // Phase 2b: the report is the honesty anchor for the compatibility claim,
    // so it gets a test that fails when it lies. Runs on the DEFAULT (report-on)
    // path -- the thing --no-report suppresses -- not the harness opt-out.
    let report =
        std::env::temp_dir().join(format!("zeo-gems-test-{}.json", std::process::id()));
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
    assert!(json.contains(r#""optparse": {"by": "bundled-gem""#), "{json}");
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
    // Phase 4: the classification behind `cargo xtask gem-compat`, over the
    // self-contained fixture store -- pure Ruby compiles, a native-extension
    // gem and a precompiled-only gem are both native-unsupported.
    use zeo::GemCompatOutcome;
    let store =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gem_store/store");
    let entries = zeo::gem_compat(&store, &store.join("Gemfile.lock")).unwrap();
    let outcome = |n: &str| entries.iter().find(|e| e.name == n).unwrap().outcome.clone();

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
    let sweep_outcome = |n: &str| swept.iter().find(|e| e.name == n).map(|e| e.outcome.clone());
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
        result.stderr.contains("uncaught exception"),
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

// ---- Phase 14.1: require/require_relative/load compile-time splicing ----
// Every positive-path expectation below was oracle-verified against real
// `ruby` (4.0.5) first, per this project's standing convention.

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
            (
                "main.rb",
                "require_relative \"ca\"\nputs \"main done\"\n",
            ),
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
fn autoload_with_a_dynamic_feature_is_a_clean_compile_error() {
    // The splice target must be compile-time-known; a computed path that
    // isn't the `File.expand_path(..., __dir__)` idiom is a clean rejection
    // (like a non-top-level `require`), not a silently-undefined constant.
    let err = zeo::compile_to_rust("autoload :X, some_method_call").unwrap_err();
    assert!(err.contains("must resolve at compile time"), "unexpected error: {err}");
}

#[test]
fn missing_require_is_a_compile_error_with_crubys_message() {
    let err = compile_project(
        &[("main.rb", "require \"definitely_missing\"\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(
        err.contains("cannot load such file -- definitely_missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn non_literal_and_non_top_level_requires_are_clean_compile_errors() {
    let err = compile_project(
        &[("main.rb", "name = \"x\"\nrequire name\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(err.contains("non-literal"), "unexpected error: {err}");

    // Inside a method body -- reaches lower_node's rejection.
    let err = zeo::compile_to_rust("def m\n  require \"x\"\nend\n").unwrap_err();
    assert!(
        err.contains("only supported as a top-level statement"),
        "unexpected error: {err}"
    );

    // Inside begin/rescue -- the optional-dependency idiom is a LOUD
    // compile error under compile-time resolution, never a silent skip.
    let err = zeo::compile_to_rust(
        "begin\n  require \"optional_dep\"\nrescue LoadError\nend\n",
    )
    .unwrap_err();
    assert!(
        err.contains("only supported as a top-level statement"),
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
fn a_feature_provided_by_two_packages_is_a_loud_ambiguity_error() {
    // Mirrors RubyGems' own `Gem::LoadError "found in multiple gems"` --
    // stricter than silent $LOAD_PATH-order shadowing.
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

// ---- Phase 14.3: base64 native-Rust package ----


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
    // The ext require-gate (#92): a require-gated builtin's constant
    // (`Base64`, gated by `"base64"`) is INVISIBLE until `require "base64"`
    // activates it -- referencing it un-required raises `NameError:
    // uninitialized constant Base64`, oracle-verified against ruby 4.0.5
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

// ---- Phase 14.4: the set pure-Ruby package + the dispatch fixes it forced ----

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

// -- Phase 18: Ruby::Box. Oracle: `RUBY_BOX=1 ruby -W:no-experimental`
// (verified per the plan's research contract; where our AOT model
// deliberately diverges -- handle inspect suffix, compile-time rejections
// -- the expectation below states OUR documented behavior).

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

/// The clean rejections: `Ruby::Box.current`-family reflection, box
/// operations outside their recognized positions, and expression-position
/// eval defining classes.
#[test]
fn ruby_box_rejections_are_clean_errors() {
    let err = zeo::compile_to_rust("p Ruby::Box.current\n").unwrap_err();
    assert!(err.contains("no compile-time meaning"), "{err}");
    let err = zeo::compile_to_rust("box = Ruby::Box.new\nx = [box.require(\"f\")]\n")
        .unwrap_err();
    assert!(err.contains("top-level statement"), "{err}");
    let err =
        zeo::compile_to_rust("box = Ruby::Box.new\nv = box.eval(\"class X; end\")\n")
            .unwrap_err();
    assert!(err.contains("class"), "{err}");
    let err = zeo::compile_to_rust("box = Ruby::Box.new\nbox.eval(1)\n").unwrap_err();
    assert!(err.contains("non-literal"), "{err}");
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
