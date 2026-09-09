#[test]
fn an_alias_inside_class_of_an_object_lowers() {
    // `class << Foo; alias greet orig; end` aliases a method on Foo's singleton
    // class (ipaddr's `class << IPSocket; alias getaddress_orig getaddress`).
    // It lowers to `Foo.singleton_class.alias_method(:greet, :orig)` and
    // COMPILES -- which is what the rubygems graph needs (this construct is
    // runtime-dead there). Its RUNTIME behavior depends on class methods being
    // reachable as singleton-class instance methods, which zeo's static
    // class-method model doesn't yet provide; tracked separately.
    let src = r#"
        class Foo
          def self.orig; "hi"; end
        end
        class << Foo
          alias greet orig
        end
    "#;
    assert!(
        zeo::check_program(src).is_ok(),
        "class << obj with an alias should compile"
    );
}

#[test]
fn class_of_an_object_with_a_self_body_lowers() {
    // `class << obj; self; end` -- the idiom that RETURNS the object's singleton
    // class (bundler's `def gem_class; class << Gem; self; end; end`). It lowers
    // to `obj.singleton_class` and COMPILES (runtime-dead in the rubygems graph;
    // `Class#singleton_class` fidelity is tracked separately).
    let src = r#"
        class Foo; end
        def gem_class; class << Foo; self; end; end
    "#;
    assert!(
        zeo::check_program(src).is_ok(),
        "class << obj with a self body should compile"
    );
}

/// The `needs_prism_runtime` verdict decides whether the binary carries the
/// parser and the five prism-backed `RubyVM` tables. This asserts the
/// decision itself (not just that programs run), because a false negative
/// would ship a binary whose `eval` is a `NotImplementedError` stub -- or,
/// for `require "prism"`, one that does not LINK -- and a false positive
/// drags 187,008 bytes into a binary that never reaches them.
#[test]
fn needs_prism_runtime_selects_the_runtime_variant() {
    let needs = |src: &str| {
        zeo::analyze_program(src, &Default::default())
            .expect("compiles")
            .compiler
            .needs_prism_runtime()
    };

    // No eval anywhere -> lean.
    assert!(!needs("puts 1"));
    // EVERY eval is a run-time one, a literal source included: a
    // compile-time splice would report the enclosing file for `__FILE__`
    // and every backtrace row and ignore a magic comment written in the
    // string.
    assert!(needs(r#"puts eval("1 + 2")"#));
    // A block-form `instance_eval` runs a real block, never the VM -> lean.
    assert!(!needs("o = Object.new\no.instance_eval { 1 + 2 }\n"));

    // A dynamic (non-literal) eval reaches the runtime VM -> needs it.
    assert!(needs("s = \"1 + 2\"\neval(s)\n"));
    // ... a literal one defining a class included.
    assert!(needs(r#"eval("class Foo; end")"#));
    // A string-form `instance_eval` reaches the VM -> needs it.
    assert!(needs("o = Object.new\no.instance_eval(\"@x = 1\")\n"));
    // A string-form `class_eval`/`module_eval` reaches it the same way, and
    // the runtime row must read its string argument rather than demand a
    // block.
    assert!(needs("class Foo; end\nFoo.class_eval(\"1 + 2\")\n"));
    assert!(needs("module M; end\nM.module_eval(\"1 + 2\")\n"));
    // The BLOCK form of either still runs a real block -> lean.
    assert!(!needs("class Foo; end\nFoo.class_eval { 1 + 2 }\n"));

    // Naming a `RubyVM` parsing surface reaches prism with no eval at all.
    assert!(needs("puts RubyVM::AbstractSyntaxTree.parse(\"1\").type\n"));

    // The eval half asks the NARROWED answer, so what `analyze` proved is
    // not an eval site is not one here either. A receiverless call to a
    // method of the enclosing class ...
    assert!(!needs("def load(x) = x\nputs load(1)\n"));
    // ... and a deferred `require` this compile emitted as a UNIT: the
    // compiled-in feature answers it, so no compiler and no parser.
    assert!(!needs("def lazy = require \"prettyprint\"\nputs lazy\n"));
    // A feature no unit answers, and a COMPUTED target, both reach the
    // on-disk tier and so keep both.
    assert!(needs(
        "def lazy = require \"no_such_feature_anywhere\"\nputs lazy\n"
    ));
    assert!(needs(
        "def lazy(n) = require n\nputs lazy(\"prettyprint\")\n"
    ));
}

// ---------------------------------------------------------------------------
// Runtime string `eval` / `instance_eval`, which zeo compiles.
//
// Every source below is held in a VARIABLE (or built with `.dup`/`+`), so it is
// NOT a string literal and therefore runs through the runtime `eval` (a
// tree-walking interpreter over prism), not the compile-time inline path a
// string literal takes. That is the surface these tests are here to cover.
// ---------------------------------------------------------------------------

// ---- a guarded `def` and the positional-install pass ----
//
// `analyze::dyn_defs` gives every `def` in a class body that installs methods
// at run time its own document position. A `def` nested in an `if` branch
// registers a method-history row and leaves NO site record, so the two lists
// it pairs by order describe different bodies -- and pairing them anyway
// handed bundler's universal-arch `Gem::BasicSpecification#extensions_dir`
// the position of rubygems' real one, on every machine that is not
// universal. These pin both halves of the rule: the pairing must refuse a
// count mismatch, and a guarded body must never be installed unconditionally.
