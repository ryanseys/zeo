use crate::support::{run_ruby, compile_packages};

#[test]
fn parse_error_is_a_clean_error_not_a_panic() {
    let err = spinelc::compile_to_rust("def foo(\n").unwrap_err();
    assert!(
        err.contains("parse error"),
        "expected a parse error, got: {err}"
    );
}

#[test]
fn eval_of_invalid_literal_source_raises_a_catchable_syntax_error() {
    // A literal `eval("...")` whose source doesn't parse no longer fails the
    // COMPILE (#97 stage 2): it falls through to the runtime eval VM and raises
    // a catchable SyntaxError, exactly as CRuby does.
    let result = run_ruby(r#"begin; eval("1 +"); rescue SyntaxError; puts "caught"; end"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught\n");
}

#[test]
fn eval_of_a_class_definition_is_not_a_compile_error() {
    // The inline path can't express a top-level `class`/`def`, so a literal
    // `eval("class Foo; end")` falls through to the runtime VM instead of
    // failing the compile. (Actually DEFINING a class inside eval is a later
    // increment; the point here is that it compiles.)
    assert!(spinelc::compile_to_rust(r#"eval("class Foo; end")"#).is_ok());
}

#[test]
fn raise_with_a_bare_class_defaults_the_message_to_the_class_name() {
    // No `rescue` exists yet (Phase 9) -- assert the UNCAUGHT path instead:
    // an unhandled `raise MyError` (no explicit message) exits 1 with the
    // class's own name as the message (no runtime `self.class` reflection
    // needed -- see `codegen::expr::emit_raise_value`'s docs).
    let result = run_ruby(
        r#"
        class MyError < StandardError
        end
        class Box
          def check
            raise MyError
          end
        end
        Box.new.check
        "#,
    );
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("MyError"),
        "expected the default message to be the class's own name, got: {}",
        result.stderr
    );
}

#[test]
fn raise_with_an_explicit_message_and_class() {
    let result = run_ruby(
        r#"
        class MyError < StandardError
        end
        class Risky
          def check(n)
            raise MyError, "bad value: #{n}" if n < 0
            n * 2
          end
        end
        r = Risky.new
        puts r.check(5)
        puts r.check(-1)
        "#,
    );
    assert!(!result.status.success());
    assert_eq!(result.stdout, "10\n");
    assert!(
        result.stderr.contains("bad value: -1"),
        "expected the explicit message in stderr, got: {}",
        result.stderr
    );
}

#[test]
fn raise_of_a_plain_string_is_an_implicit_runtime_error() {
    let result = run_ruby(
        r#"
        class Risky
          def check
            raise "plain string error"
          end
        end
        Risky.new.check
        "#,
    );
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("plain string error"),
        "expected the string message in stderr, got: {}",
        result.stderr
    );
}

#[test]
fn bare_raise_with_no_active_rescue_constructs_a_runtime_error() {
    // Bare `raise` (re-raise) outside any active `rescue` clause -- real
    // Ruby constructs a fresh `RuntimeError` with an EMPTY message rather
    // than erroring (oracle-verified: `ruby -e 'begin; raise; rescue => e;
    // puts "[#{e.message}]"; end'` -> `"[]"`) -- see
    // `codegen::expr::emit_raise`'s docs. This used to be a clean codegen
    // panic before Phase 9's `spinel_rt::current_exception` fallback shipped.
    let result = run_ruby(
        r#"
        begin
          raise
        rescue => e
          puts "[#{e.message}]"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[]\n");
}

#[test]
fn raising_a_non_exception_is_a_type_error() {
    // `raise <non-exception>` coerces at runtime -- a String becomes a
    // RuntimeError, everything else is CRuby's TypeError -- instead of
    // panicking when the raise machinery unwraps a non-Object.
    let result = run_ruby(
        r#"
        def try
          yield
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        try { raise 42 }
        try { raise nil }
        try { raise :sym }
        begin
          raise "boom"
        rescue RuntimeError => e
          puts "RuntimeError: #{e.message}"
        end
        mixed = [Object.new, "msg"]
        begin
          raise mixed[1]
        rescue RuntimeError => e
          puts "poly string: #{e.message}"
        end
        begin
          raise mixed[0]
        rescue TypeError => e
          puts "poly object: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "TypeError: exception class/object expected\n\
         TypeError: exception class/object expected\n\
         TypeError: exception class/object expected\n\
         RuntimeError: boom\n\
         poly string: msg\n\
         poly object: exception class/object expected\n"
    );
}

#[test]
fn coercion_type_errors_name_nil_true_false_as_literals_not_class_names() {
    // CRuby renders nil/true/false in a coercion TypeError as those words, not
    // `NilClass`/`TrueClass`/`FalseClass`; every other object uses its class
    // name (`Integer`, `Symbol`).
    let result = run_ruby(
        r#"
        def msg
          yield
        rescue TypeError => e
          puts e.message
        end
        msg { "" + nil }
        msg { "" + true }
        msg { "" + false }
        msg { "" + 1 }
        msg { "" + :s }
        msg { Float(nil) }
        msg { Integer(true) }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "no implicit conversion of nil into String\n\
         no implicit conversion of true into String\n\
         no implicit conversion of false into String\n\
         no implicit conversion of Integer into String\n\
         no implicit conversion of Symbol into String\n\
         can't convert nil into Float\n\
         can't convert true into Integer\n"
    );
}

#[test]
fn match_predicate_one_liner_is_a_boolean_that_never_raises() {
    let result = run_ruby(
        r#"
        if [1, 2] in [Integer, Integer]
          puts "matched"
        end
        if [1, "x"] in [Integer, Integer]
          puts "should not print"
        else
          puts "did not match"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "matched\ndid not match\n");
}

#[test]
fn case_in_with_no_matching_arm_and_no_else_raises() {
    let result = run_ruby("case 5\nin String\n  puts \"no\"\nend\n");
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uncaught exception: no matching pattern"),
        "stderr: {}",
        result.stderr
    );
}

// --- Phase 9: exceptions (begin/rescue/else/ensure/retry, raise, custom
// hierarchies), oracle-verified against real `ruby` first. `e`'s method
// calls (`e.message`) always go through `.send(:message)` -- a rescue
// binding is deliberately never narrowed to a concrete class (unlike a
// pattern's `Integer => n`): unlike a builtin primitive's runtime tag check,
// dispatch there is Rust `downcast::<T>()`-based, which only succeeds
// against the EXACT concrete struct, and `rescue StandardError => e` must
// also match any raised SUBCLASS instance -- see
// `codegen::exceptions::emit_rescue_chain`'s docs.

#[test]
fn begin_rescue_catches_and_ensure_always_runs() {
    let result = run_ruby(
        r#"
        begin
          puts "try"
          raise ArgumentError, "bad"
        rescue ArgumentError => e
          puts "caught: #{e.send(:message)}"
        ensure
          puts "ensure ran"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "try\ncaught: bad\nensure ran\n");
}

#[test]
fn rescuing_a_middle_class_catches_a_raised_leaf_subclass() {
    let result = run_ruby(
        r#"
        class AppError < StandardError
        end
        class ValidationError < AppError
        end

        begin
          raise ValidationError, "bad input"
        rescue AppError => e
          puts "caught as AppError: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught as AppError: bad input\n");
}

/// Reopening a NATIVE exception class (D3): a reopen `def` ADDS a method that
/// reaches every subclass, and OVERRIDES an existing native method everywhere.
/// The native message lives in a hidden slot, so a reopened `message` reading
/// `@message` sees nil (CRuby parity) -- here it reads `to_s`.
#[test]
fn native_exception_reopen_adds_and_overrides() {
    let result = run_ruby(
        r#"
        class StandardError
          def code
            42
          end
        end
        class Exception
          def message
            "patched: " + to_s
          end
        end
        begin
          raise ArgumentError, "bad value"
        rescue => e
          puts e.code
          puts e.message
          puts e.is_a?(StandardError)
        end
        begin
          raise "plain"
        rescue => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "42\npatched: bad value\ntrue\npatched: plain\n"
    );
}

/// A raised native exception exposes NO `@message` ivar (CRuby stores it in a
/// hidden slot): `instance_variables` is empty and `@message` reads nil.
#[test]
fn native_exception_hides_its_message_ivar() {
    let result = run_ruby(
        r#"
        begin
          raise ArgumentError, "boom"
        rescue => e
          p e.instance_variables
          p e.instance_variable_get(:@message)
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[]\nnil\nboom\n");
}

/// A USER exception subclass is backed by the native `RubyException` (D3): a
/// custom ivar, `super` into the native `initialize` (which stores the message
/// in its hidden slot), and reflection all match CRuby -- `instance_variables`
/// is `[:@code]` only (NOT `@message`), and `@message` reads nil while
/// `#message` returns the super-provided text.
#[test]
fn user_exception_subclass_uses_native_representation() {
    let result = run_ruby(
        r##"
        class MyErr < StandardError
          def initialize(code)
            @code = code
            super("boom #{code}")
          end
          def code
            @code
          end
        end

        e = MyErr.new(42)
        puts e.message
        puts e.code
        puts e.is_a?(StandardError)
        p e.instance_variables
        p e.instance_variable_get(:@message)
        p e.instance_variable_get(:@code)
        begin
          raise MyErr, "direct"
        rescue StandardError => ex
          puts "#{ex.class}: #{ex.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "boom 42\n42\ntrue\n[:@code]\nnil\n42\nMyErr: boom direct\n"
    );
}

/// A user exception subclass with NO `initialize` inherits the native default:
/// `Plain.new(msg)` stores the message in the hidden slot, so `#message`
/// returns it and `instance_variables` is empty. Multi-level `super` chains
/// (`B < A < StandardError`, bare `super` forwarding) also resolve natively.
#[test]
fn user_exception_subclass_super_chain_and_inherited_initialize() {
    let result = run_ruby(
        r#"
        class Plain < RuntimeError
        end
        p1 = Plain.new("hi")
        puts p1.message
        p p1.instance_variables

        class A < StandardError
          def initialize(msg = "a-default")
            super
          end
        end
        class B < A
          def initialize
            super()
            @tag = "b"
          end
          def tag; @tag; end
        end
        b = B.new
        puts b.message
        puts b.tag
        puts b.is_a?(A)

        class Done < StopIteration
        end
        puts Done.new("stop").message
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hi\n[]\na-default\nb\ntrue\nstop\n"
    );
}

/// An exception subclass whose `initialize` takes KEYWORD arguments (D3): the
/// runtime construction path (`construct_by_class_id`) must carry keywords as
/// the trailing-Hash the trampoline expects, or `code:` would default. Two
/// sibling subclasses -- neither's `message` param pinned to a String -- also
/// exercise the poly `super(message)` coercion into the native message slot.
#[test]
fn user_exception_subclass_keyword_initialize() {
    let result = run_ruby(
        r#"
        class AError < StandardError
          attr_reader :code
          def initialize(message, code: 3)
            super(message)
            @code = code
          end
        end
        class BError < StandardError
          attr_reader :level
          def initialize(message, level = 7)
            super(message)
            @level = level
          end
        end
        a = AError.new("boom", code: 9)
        puts a.code
        puts a.message
        b = BError.new("bad", 5)
        puts b.level
        puts b.message
        puts AError.new("d").code
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "9\nboom\n5\nbad\n3\n");
}

/// A `def self.x` on an exception subclass emits into a `pub mod` (no struct to
/// attach an `impl` to), and a bare `new` inside it constructs via the runtime
/// (D3) -- `Self.new` there yields the native `RubyException`, not a `new_handle`.
#[test]
fn user_exception_subclass_class_method_constructs_via_runtime() {
    let result = run_ruby(
        r#"
        class D < StandardError
          def self.build(n)
            new("built-#{n}")
          end
        end
        e = D.build(3)
        puts e.message
        puts e.class
        puts e.is_a?(StandardError)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "built-3\nD\ntrue\n");
}

#[test]
fn rescue_with_multiple_classes_in_one_clause() {
    let result = run_ruby(
        r#"
        begin
          raise TypeError, "wrong type"
        rescue ArgumentError, TypeError => e
          puts "caught one of: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught one of: wrong type\n");
}

#[test]
fn bare_raise_re_raises_preserving_a_custom_exceptions_own_ivars() {
    let result = run_ruby(
        r#"
        class MyError < StandardError
          def initialize(msg, code)
            super(msg)
            @code = code
          end
          def code
            @code
          end
        end

        class Inner
          def call
            begin
              raise MyError.new("failed with code 42", 42)
            rescue MyError => e
              puts "inner saw code #{e.send(:code)}"
              raise
            end
          end
        end

        begin
          Inner.new.call
        rescue MyError => e
          puts "outer saw code #{e.send(:code)}"
          puts "outer message: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "inner saw code 42\nouter saw code 42\nouter message: failed with code 42\n"
    );
}

#[test]
fn return_from_a_rescue_body_still_runs_ensure_exactly_once() {
    let result = run_ruby(
        r#"
        class ReturnTester
          def m
            begin
              raise "x"
            rescue
              return "returned"
            ensure
              puts "ensure ran before return"
            end
          end
        end
        puts ReturnTester.new.m
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ensure ran before return\nreturned\n");
}

#[test]
fn retry_loop_runs_ensure_exactly_once_not_per_attempt() {
    let result = run_ruby(
        r#"
        class Attempt
          def initialize
            @attempts = 0
          end
          def run
            begin
              @attempts += 1
              raise "fail" if @attempts < 3
              puts "succeeded after #{@attempts} attempts"
            rescue
              retry if @attempts < 3
            ensure
              puts "ensure ran, attempts=#{@attempts}"
            end
          end
        end
        Attempt.new.run
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "succeeded after 3 attempts\nensure ran, attempts=3\n");
}

#[test]
fn nested_retry_only_restarts_the_innermost_begin() {
    let result = run_ruby(
        r#"
        outer_runs = 0
        inner_attempts = 0
        begin
          outer_runs += 1
          begin
            inner_attempts += 1
            raise "x" if inner_attempts < 2
            puts "inner ok after #{inner_attempts}"
          rescue
            retry if inner_attempts < 2
          end
          puts "outer ran #{outer_runs} times"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "inner ok after 2\nouter ran 1 times\n");
}

#[test]
fn method_level_implicit_rescue_with_no_explicit_begin_end() {
    // `def m; ...; rescue => e; ...; end` -- `DefNode::body()` is directly a
    // `BeginNode` with no `begin_keyword_loc` in this shape (confirmed via
    // `Prism.parse`), flowing through the SAME `HirNode::Begin` lowering as
    // an explicit `begin`.
    let result = run_ruby(
        r#"
        class Worker
          def safe
            raise "oops"
          rescue => e
            "handled: #{e.send(:message)}"
          end
        end
        puts Worker.new.safe
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "handled: oops\n");
}

#[test]
fn endless_method_rescue_modifier() {
    let result = run_ruby(
        r#"
        class Unsafe
          def op(n)
            raise "negative" if n < 0
            n * 2
          end
        end
        class Calc
          def safe_op(n) = Unsafe.new.op(n) rescue -1
        end
        c = Calc.new
        puts c.safe_op(5)
        puts c.safe_op(-5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n-1\n");
}

#[test]
fn assignment_rescue_modifier() {
    let result = run_ruby(
        r#"
        class TopRisk
          def risky_top
            raise "bad"
          end
        end
        x = TopRisk.new.risky_top rescue "fallback"
        puts x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fallback\n");
}

#[test]
fn unmatched_rescue_class_propagates_to_an_outer_rescue() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise TypeError, "inner"
          rescue ArgumentError
            puts "wrong handler"
          end
        rescue TypeError => e
          puts "outer caught: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "outer caught: inner\n");
}

#[test]
fn break_next_from_inside_begin_rescue_nested_in_a_real_escaping_block_works() {
    // `Array#each`/general Enumerable iteration isn't implemented (Phase 3's
    // documented scope-cut), so a custom `yield`-based method is the
    // supported way to attach a real escaping block here. The begin/rescue
    // closure boundary (see `codegen::exceptions`'s module docs) needs no
    // special handling: the block passed to `each_num` is ALREADY a real
    // escaping `Proc` (its own closure boundary, `in_real_proc` already
    // true), so a `next` inside the nested `begin`'s rescue clause raises
    // the exact same `Signal` it already would have -- caught by the Proc's
    // own wrapper loop, not by the (irrelevant here) native-loop rejection
    // check.
    let result = run_ruby(
        r#"
        class Each3
          def each_num
            yield 1
            yield 2
            yield 3
          end
        end

        Each3.new.each_num do |i|
          begin
            raise "boom" if i == 2
            puts "ok #{i}"
          rescue
            next
          ensure
            puts "ensure #{i}"
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok 1\nensure 1\nensure 2\nok 3\nensure 3\n");
}

#[test]
fn break_inside_begin_rescue_targets_the_enclosing_native_loop() {
    // A bare `break`/`next`/`redo` inside a `begin`/`rescue`/`else` clause
    // targeting a native loop OUTSIDE the `begin` (Batch H2). The `begin`
    // expression is spliced INLINE in the loop body, so its final settling
    // translates the bubbled `Signal` into the loop's own literal jump -- no
    // loop-body closure needed (see `codegen::exceptions`'s module docs). A
    // loop written INSIDE the `begin` is unaffected (the previous test).
    let result = run_ruby(
        r#"
        i = 0
        while i < 5
          begin
            break if i == 3
            puts "body #{i}"
          rescue
          end
          i += 1
        end
        puts "after"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "body 0\nbody 1\nbody 2\nafter\n");
}

#[test]
fn next_inside_begin_rescue_continues_the_enclosing_native_loop() {
    // `next` from a `rescue` clause -> `continue` the enclosing loop (H2).
    let result = run_ruby(
        r#"
        i = 0
        while i < 5
          i += 1
          begin
            raise "x" if i.even?
            puts "odd #{i}"
          rescue
            next
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "odd 1\nodd 3\nodd 5\n");
}

#[test]
fn ensure_still_runs_when_break_crosses_out_of_a_begin() {
    // The bubbled `break` is translated only AFTER `ensure` runs, exactly once
    // -- the loop-crossing settle sits below the ensure block (H2).
    let result = run_ruby(
        r#"
        i = 0
        while i < 4
          begin
            break if i == 2
            puts "b#{i}"
          ensure
            puts "e#{i}"
          end
          i += 1
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "b0\ne0\nb1\ne1\ne2\n");
}

#[test]
fn break_in_begin_still_leaves_a_raise_free_to_propagate() {
    // The loop-crossing settle only translates Break/Next/Redo; a re-raised
    // exception still `?`-propagates out to the outer handler (H2).
    let result = run_ruby(
        r#"
        begin
          i = 0
          while i < 3
            begin
              raise "boom" if i == 1
            rescue => e
              raise "rethrow #{e.message}"
            end
            i += 1
          end
        rescue => e
          puts "caught: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught: rethrow boom\n");
}

// --- Phase 9 continued: deeper edge-case and composition coverage, added
// after the initial batch above per an explicit request for more
// comprehensive tests. Every scenario oracle-verified against real `ruby`
// first, per this project's established convention.

#[test]
fn bare_rescue_does_not_catch_a_script_error_level_exception() {
    // A bare `rescue` matches `StandardError` and below ONLY -- it must NOT
    // catch a raised `ScriptError` (a sibling branch of the hierarchy, both
    // direct children of `Exception`). This is the real semantic bare
    // `rescue`'s default narrows to `StandardError`, not `Exception`.
    let result = run_ruby(
        r#"
        begin
          begin
            raise ScriptError, "script problem"
          rescue => e
            puts "should not print: #{e.send(:message)}"
          end
        rescue ScriptError => e
          puts "outer caught ScriptError: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "outer caught ScriptError: script problem\n");
}

#[test]
fn rescue_exception_class_explicitly_catches_a_script_error() {
    let result = run_ruby(
        r#"
        begin
          raise ScriptError, "script problem"
        rescue Exception => e
          puts "caught via Exception: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught via Exception: script problem\n");
}

#[test]
fn return_from_ensure_overrides_return_from_the_begin_body() {
    // Real, tricky Ruby semantic: a `return` inside `ensure` wins over a
    // `return` already in flight from `begin`'s own body -- not just "ensure
    // runs afterward", it actually REPLACES the method's return value.
    // Falls out for free here: `ensure`'s own statements are ordinary Rust
    // code (not wrapped in the begin/rescue closure boundary -- see
    // `codegen::exceptions`'s module docs), so a literal `return` inside it
    // exits the enclosing method directly, superseding whatever `__final`
    // already held.
    let result = run_ruby(
        r#"
        class M
          def m
            begin
              return "from begin"
            ensure
              return "from ensure"
            end
          end
        end
        puts M.new.m
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from ensure\n");
}

#[test]
fn raise_inside_ensure_replaces_the_original_exception() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise "original"
          ensure
            raise "from ensure"
          end
        rescue => e
          puts "caught: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught: from ensure\n");
}

#[test]
fn nested_begin_ensure_runs_inner_before_outer() {
    let result = run_ruby(
        r#"
        begin
          begin
            puts "inner body"
          ensure
            puts "inner ensure"
          end
        ensure
          puts "outer ensure"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "inner body\ninner ensure\nouter ensure\n");
}

#[test]
fn re_raise_preserves_the_exact_same_object_not_a_copy() {
    // Proves re-raise doesn't construct a fresh exception -- mutating an
    // ivar (via an `attr_accessor`-generated setter) INSIDE the inner
    // `rescue`, then bare `raise`-ing, must be visible to the OUTER
    // `rescue` reading the same ivar back. Constructed and raised in ONE
    // expression (`raise Tagged.new(...)`), not first assigned to a named
    // local -- a named local assigned an `Object`-typed value inside a
    // `begin`'s body (or any branching construct) hits a real, pre-existing,
    // Phase-9-independent codegen gap when that local's type has to widen
    // to `Poly` across branches (confirmed to affect plain `if`/`else` too,
    // not something this phase introduced or is responsible for fixing).
    let result = run_ruby(
        r#"
        class Tagged < StandardError
          def initialize(msg, tag)
            super(msg)
            @tag = tag
          end
          attr_accessor :tag
        end
        begin
          begin
            raise Tagged.new("x", "initial")
          rescue Tagged => e
            e.send(:tag=, "mutated")
            raise
          end
        rescue Tagged => e
          puts "tag: #{e.send(:tag)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "tag: mutated\n");
}

#[test]
fn begin_rescue_used_as_an_expressions_value() {
    let result = run_ruby(
        r#"
        x = begin
          raise "bad"
        rescue
          -1
        end
        puts x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "-1\n");
}

#[test]
fn begin_ensure_with_no_rescue_clause_still_runs_ensure_and_propagates() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise "boom"
          ensure
            puts "ensure ran"
          end
        rescue => e
          puts "caught outside: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ensure ran\ncaught outside: boom\n");
}

#[test]
fn ensure_runs_when_no_exception_is_raised_and_there_is_no_rescue_at_all() {
    let result = run_ruby(
        r#"
        begin
          puts "no exception"
        ensure
          puts "ensure always"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "no exception\nensure always\n");
}

#[test]
fn three_rescue_clauses_skips_non_matching_ones_in_order() {
    let result = run_ruby(
        r#"
        begin
          raise RangeError, "oops"
        rescue ArgumentError
          puts "arg"
        rescue TypeError
          puts "type"
        rescue RangeError => e
          puts "range: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "range: oops\n");
}

#[test]
fn plain_string_raise_is_caught_by_an_explicit_runtime_error_rescue() {
    let result = run_ruby(
        r#"
        begin
          raise "just a string"
        rescue RuntimeError => e
          puts "caught runtime: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught runtime: just a string\n");
}

#[test]
fn case_in_pattern_matching_inside_a_rescue_body() {
    // Composes Phase 8 (pattern matching) with Phase 9 (exceptions) --
    // `e.send(:message)` is a `Poly` expression, matched via `case/in`'s own
    // `#deconstruct`-independent `ClassCheck`/`Capture` path.
    let result = run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          case e.send(:message)
          in String => s
            puts "matched string: #{s}"
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "matched string: boom\n");
}

#[test]
fn times_loop_containing_begin_rescue_with_no_crossing_control_flow_still_works() {
    // A native loop CONTAINING a `begin`/`rescue` (as opposed to a `begin`
    // containing a bare `break`/`next` TARGETING an outer loop, the
    // rejected shape) is completely unaffected -- the `.times` loop's own
    // control flow doesn't cross the begin/rescue closure boundary at all
    // here, so it just runs normally, once per iteration.
    let result = run_ruby(
        r#"
        3.times do |i|
          begin
            raise "x" if i == 1
            puts "ok #{i}"
          rescue
            puts "rescued #{i}"
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok 0\nrescued 1\nok 2\n");
}

#[test]
fn class_method_begin_rescue_ensure_with_return() {
    // Exercises `codegen::mod::emit_class_method_fn`'s own `Signal::Return`
    // catch (added specifically for `Begin` nodes inside a class method --
    // class methods can't contain an escaping block at all, so `Begin` was
    // the only possible trigger there).
    let result = run_ruby(
        r#"
        class Factory
          def self.build(fail_it)
            begin
              raise "nope" if fail_it
              "built"
            rescue
              return "fallback"
            ensure
              puts "factory ensure"
            end
          end
        end
        puts Factory.build(false)
        puts Factory.build(true)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "factory ensure\nbuilt\nfactory ensure\nfallback\n");
}

#[test]
fn retry_and_ensure_combined_inside_an_enclosing_while_loop() {
    // The `begin`'s own `retry` restarts just its OWN body (not the `while`
    // loop), and `ensure` runs once per `while` ITERATION (twice total,
    // once per `i`) -- not once per `retry` attempt within an iteration.
    let result = run_ruby(
        r#"
        total_ensure = 0
        i = 0
        while i < 2
          attempts = 0
          begin
            attempts += 1
            raise "x" if attempts < 2
          rescue
            retry if attempts < 2
          ensure
            total_ensure += 1
          end
          i += 1
        end
        puts total_ensure
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn unrescued_raise_propagates_through_multiple_method_call_frames() {
    // Three separate objects (not sibling methods on one class) --
    // implicit-self calls to a sibling method aren't supported yet
    // (a separate, pre-existing, documented gap from Phase 5/6), so each
    // level calls the next via an explicit receiver instead. Still a
    // genuine 3-frame unwind, crossing object boundaries too.
    let result = run_ruby(
        r#"
        class Level3
          def run
            raise "deep failure"
          end
        end
        class Level2
          def run
            Level3.new.run
          end
        end
        class Level1
          def run
            Level2.new.run
          end
        end
        begin
          Level1.new.run
        rescue => e
          puts "caught from deep: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught from deep: deep failure\n");
}

#[test]
fn array_index_assign_out_of_range_raises_index_error_not_a_panic() {
    // Before this fix, a negative out-of-range `Array#[]=` index was an
    // unconditional Rust `panic!` -- a real `IndexError` (catchable) is
    // constructed instead now, and the array is otherwise
    // unaffected (an in-range negative-from-end or growing-positive index
    // still works exactly as before).
    let result = run_ruby(
        r#"
        a = [1, 2, 3]
        begin
          a[-10] = :x
        rescue IndexError => e
          puts "caught"
        end
        a[5] = :y
        puts a.length
        puts a[5]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught\n6\ny\n");
}

#[test]
fn poly_receiver_call_with_unknown_keyword_raises_at_runtime() {
    // The other old silent-drop site: kwargs on a Poly receiver now ride
    // the G2 convention; an undefined method stays a real NoMethodError.
    let result = run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          begin
            e.foo(bar: 1)
          rescue NoMethodError
            puts "no method, kwargs carried"
          end
        end
        "#,
    );
    assert_eq!(result.stdout, "no method, kwargs carried\n");
}

#[test]
fn respond_to_on_a_poly_typed_rescue_binding() {
    // `e` (a rescue clause's exception binding) is never narrowed to a
    // concrete class (see `codegen::exceptions`'s docs) -- exercises
    // `respond_to?`'s runtime `class_id()` fallback path, not the
    // statically-known-class one the test above exercises.
    let result = run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          puts e.respond_to?(:message)
          puts e.respond_to?(:not_a_thing)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\n");
}

#[test]
fn private_method_called_with_an_explicit_receiver_raises_at_runtime() {
    // A visibility violation is RUNTIME behavior in Ruby, not a syntax
    // error: it raises NoMethodError when the call actually runs, so it can
    // be rescued and an unreachable one stays silent. Message shape and
    // both behaviors oracle-verified.
    let result = run_ruby(
        r#"
        class Box
          def initialize
            @x = 1
          end

          private

          def helper
            2
          end
        end

        b = Box.new
        begin
          b.helper
        rescue NoMethodError => e
          puts e.message
        end

        def never_runs(b)
          b.helper
        end
        puts "still here"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "private method 'helper' called for an instance of Box
still here
"
    );
}

#[test]
fn protected_method_called_from_outside_any_related_class_raises_at_runtime() {
    // Same rule as the private case above -- a runtime NoMethodError.
    let result = run_ruby(
        r#"
        class Money
          def initialize(amount)
            @amount = amount
          end

          protected

          def amount
            @amount
          end
        end

        a = Money.new(10)
        begin
          a.amount
        rescue NoMethodError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "protected method 'amount' called for an instance of Money
"
    );
}

#[test]
fn lambda_enforces_strict_arity_via_argument_error() {
    let result = run_ruby(
        r#"
        f = ->(x, y) { x + y }
        begin
          f.call(1)
        rescue ArgumentError => e
          puts "caught: #{e.message}"
        end
        puts f.call(1, 2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "caught: wrong number of arguments (given 1, expected 2)\n3\n"
    );
}

#[test]
fn unset_constant_raises_a_name_error() {
    let result = run_ruby(
        r#"
        begin
          puts UNDEFINED_CONST
        rescue NameError => e
          puts "caught: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught: uninitialized constant UNDEFINED_CONST\n");
}

#[test]
fn subclassing_an_unsupported_built_in_type_is_a_clean_error() {
    // D3 opened subclassing for Array/String/Hash (payload), Numeric (plain
    // object), and the immediates (registry-only). `Range` stays rejected --
    // it has no runtime constructor -- so this remains a clean compile error.
    let err = spinelc::compile_to_rust(
        r#"
        class MyRange < Range
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("subclassing the built-in type"), "{err}");
}

#[test]
fn an_invalid_interpolated_pattern_raises_a_catchable_regexp_error() {
    // A STATIC (non-interpolated) invalid pattern is a real, uncatchable
    // `SyntaxError` at parse time in real Ruby -- this spike defers that
    // check to construction time uniformly (a documented, narrower-timing
    // approximation, see `hir::HirNode::RegexpLit`'s docs), so only the
    // INTERPOLATED case (genuinely runtime-only in real Ruby too) is
    // oracle-verified here as a rescuable exception.
    let result = run_ruby(
        r#"
        bad = "("
        begin
          r = /#{bad}/
          puts "no error"
        rescue RegexpError => e
          puts "regexp error"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "regexp error\n");
}

#[test]
fn a_bare_regexp_literal_used_as_an_implicit_condition_is_a_clean_lowering_error() {
    let err = spinelc::compile_to_rust(
        r#"
        if /foo/
          puts "matched"
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("implicit condition"), "{err}");
}

#[test]
fn encoding_surface_reports_transcodes_and_raises() {
    // The Encoding engine: a string is bytes + an encoding, queryable and
    // transcodable, with the Encoding class and its constants. `__ENCODING__`
    // answers the script encoding (UTF-8). Cross-checked against ruby 4.0.5.
    let result = run_ruby(
        r##"
        p "hello".encoding
        p __ENCODING__
        p "€".bytesize
        p "€".length
        p "hello".ascii_only?
        p "€".ascii_only?
        p Encoding::UTF_8.name
        p Encoding::ASCII_8BIT.names
        p Encoding.find("BINARY")
        p Encoding.default_external
        raw = "€".b
        p raw.encoding
        p raw.length
        mis = "€".dup.force_encoding("US-ASCII")
        p mis.valid_encoding?
        latin = "café".encode(Encoding::ISO_8859_1)
        p latin.encoding
        p latin.bytes
        p latin.encode(Encoding::UTF_8) == "café"
        p "café".encode(Encoding::US_ASCII, undef: :replace)
        p "a<b>&c".encode(Encoding::US_ASCII, xml: :text)
        begin
          "café".encode(Encoding::US_ASCII)
        rescue Encoding::UndefinedConversionError => e
          puts "raised #{e.class}"
        end
        p :hi.encoding
        p :café.encoding
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<Encoding:UTF-8>\n#<Encoding:UTF-8>\n3\n1\ntrue\nfalse\n\
         \"UTF-8\"\n[\"ASCII-8BIT\", \"BINARY\"]\n\
         #<Encoding:BINARY (ASCII-8BIT)>\n#<Encoding:UTF-8>\n\
         #<Encoding:BINARY (ASCII-8BIT)>\n3\nfalse\n\
         #<Encoding:ISO-8859-1>\n[99, 97, 102, 233]\ntrue\n\
         \"caf?\"\n\"a&lt;b&gt;&amp;c\"\n\
         raised Encoding::UndefinedConversionError\n\
         #<Encoding:US-ASCII>\n#<Encoding:UTF-8>\n"
    );
}

#[test]
fn aliasing_a_genuinely_undefined_method_is_a_clean_error() {
    // An `alias`/`alias_method` whose source is defined neither in this body
    // nor anywhere in the ancestry is a clean compile error -- now raised by
    // `mro::resolve_aliases` (which resolves inherited sources), not a
    // lowering-time rejection.
    let err = spinelc::compile_to_rust(
        r#"
        class Foo
          alias bar undefined_method
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("undefined method 'undefined_method'"), "{err}");
}

#[test]
fn other_compound_assignment_operators_still_raise_on_an_undefined_constant() {
    // Regression guard for the fix above: `||=`'s new leniency must NOT
    // leak into `+=`/`&&=` -- confirmed against real `ruby` that those
    // still raise on a genuinely undefined constant. An uncaught Ruby
    // exception exits the compiled binary nonzero with a message on
    // stderr (see `uncaught_raise_with_no_rescue_anywhere_exits_with_the_message`),
    // not a Rust-level panic, so this asserts on the process output
    // directly rather than `#[should_panic]`.
    let result = run_ruby("UNDEF += 1");
    assert!(!result.status.success());
    assert!(result.stderr.contains("uninitialized constant UNDEF"), "{}", result.stderr);
}

#[test]
fn integer_division_and_modulo_by_zero_raise_a_catchable_zero_division_error() {
    // A real, previously-undetected bug: `1 / 0` crashed the ENTIRE
    // generated binary with a raw Rust panic (`spinel_rt::int_div`'s own
    // internal `/` panicking) instead of raising a catchable
    // `ZeroDivisionError` -- real Ruby's actual behavior. This affected
    // BOTH the static `Int`-`Int` fast path and the runtime-checked
    // fallback used for method-parameter operands (always `Poly`). Fixed
    // via `emit_int_div_or_mod_checked`, consulted at both call sites.
    let result = run_ruby(
        r#"
        begin
          puts(5 % 0)
        rescue ZeroDivisionError
          puts "mod zero div"
        end
        class Calc
          def divide(a, b)
            a / b
          end
        end
        begin
          puts Calc.new.divide(10, 0)
        rescue ZeroDivisionError
          puts "param zero div"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "mod zero div\nparam zero div\n");
}

#[test]
fn float_division_by_zero_returns_infinity_not_an_error() {
    // Regression guard for the fix above: `ZeroDivisionError` must stay
    // `Int`/`Int`-division-specific -- real Ruby's `Float`/`0` is
    // `Infinity`/`-Infinity`/`NaN` (IEEE semantics), never a raised
    // exception, and `float_div` already gives this for free with no
    // check needed.
    let result = run_ruby("puts(1.0 / 0.0); puts(-1.0 / 0.0)");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Infinity\n-Infinity\n");
}

// ---------------------------------------------------------------------------
// Phase 13.1: .freeze / .frozen? -- every snippet oracle-verified against
// real `ruby` first, per this project's standing convention. Semantics
// grounded in CRuby's actual implementation (see the plan's Part 11
// addendum): freeze is SHALLOW, returns self, no-ops when repeated;
// immediates and Ranges are always frozen; mutation of a frozen value
// raises a catchable FrozenError (a RuntimeError subclass) with the message
// `can't modify frozen <Class>: <inspect>`, checked at the top of every
// mutator after argument evaluation.
// ---------------------------------------------------------------------------

#[test]
fn freezing_an_array_makes_index_assignment_raise_a_catchable_frozen_error() {
    let result = run_ruby(
        r#"
        a = [1, 2, 3]
        a.freeze
        begin
          a[0] = 9
        rescue FrozenError => e
          puts e.send(:message)
        end
        puts a[0]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Array: [1, 2, 3]\n1\n");
}

#[test]
fn frozen_hash_assignment_raises_with_the_inspect_bearing_message() {
    let result = run_ruby(
        r#"
        h = { a: 1 }
        h.freeze
        begin
          h[:b] = 2
        rescue FrozenError => e
          puts e.send(:message)
        end
        puts h.length
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Hash: {a: 1}\n1\n");
}

#[test]
fn frozen_string_assignment_raises_with_the_quoted_inspect_message() {
    let result = run_ruby(
        r#"
        s = "abc"
        s.freeze
        begin
          s[0] = "z"
        rescue FrozenError => e
          puts e.send(:message)
        end
        puts s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen String: \"abc\"\nabc\n");
}

#[test]
fn ivar_write_on_a_frozen_object_raises_and_leaves_the_ivar_unchanged() {
    // The message carries the receiver's real `#<Pt:0xADDR @x=1>` inspect
    // (address normalized in-program). Semantics oracle-verified: a catchable
    // FrozenError, the ivar keeps its old value, readers still work.
    let result = run_ruby(
        r#"
        class Pt
          def initialize(x)
            @x = x
          end
          def set_x(v)
            @x = v
          end
          def x
            @x
          end
        end
        p1 = Pt.new(1)
        p1.freeze
        puts p1.frozen?
        begin
          p1.set_x(5)
        rescue FrozenError => e
          puts e.send(:message).gsub(/0x[0-9a-f]+/, "0xADDR")
        end
        puts p1.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ncan't modify frozen Pt: #<Pt:0xADDR @x=1>\n1\n"
    );
}

#[test]
fn frozen_error_is_caught_by_a_runtime_error_rescue() {
    let result = run_ruby(
        r#"
        g = [1].freeze
        begin
          g[0] = 2
        rescue RuntimeError => e
          puts "runtime-rescued"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "runtime-rescued\n");
}

#[test]
fn compound_assignment_on_a_frozen_array_raises_after_the_read() {
    // `d[0] += 1` desugars to a read (fine on a frozen array) then a `[]=`
    // (raises) -- real Ruby's own order.
    let result = run_ruby(
        r#"
        d = [1]
        d.freeze
        begin
          d[0] += 1
        rescue FrozenError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Array: [1]\n");
}

#[test]
fn multi_assignment_into_a_frozen_index_target_raises() {
    let result = run_ruby(
        r#"
        f = [1, 2]
        f.freeze
        begin
          f[0], x = 5, 6
        rescue FrozenError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Array: [1, 2]\n");
}

// ---------------------------------------------------------------------------
// Phase 13.7: `send`'s missing-method fallback raises a real, catchable
// NoMethodError (via the factory generated main() installs) instead of the
// original whole-process `exit(1)` -- which would have killed every OTHER
// running Thread over one bad dispatch. Oracle-verified.
// ---------------------------------------------------------------------------

#[test]
fn a_missing_method_via_send_raises_a_rescuable_no_method_error() {
    let result = run_ruby(
        r#"
        class Plain
          def real
            :real
          end
        end
        p1 = Plain.new
        begin
          p1.send(:nope)
        rescue NoMethodError => e
          puts "caught nope"
        end
        begin
          p1.send(:nope2)
        rescue NameError => e
          puts "caught via NameError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught nope\ncaught via NameError\n");
}

#[test]
fn exception_objects_expose_their_typed_introspection_accessors() {
    // KeyError#key, NameError#name/#receiver, NoMethodError#name/#args/#receiver,
    // UncaughtThrowError#tag/#value, and Exception#detailed_message -- populated
    // both at the raise site (a failed fetch / missing method / const miss /
    // uncaught throw) and from an explicit constructor. Byte-verified against
    // ruby 4.0.5.
    let result = run_ruby(
        r#"
        begin; {}.fetch(:sym); rescue KeyError => e; p e.key; end
        begin; {}.fetch(42); rescue KeyError => e; p e.key; end
        e = NoMethodError.new("msg", :meth, [1, 2]); p e.args; p e.name; p e.message
        p NoMethodError.new("m2", :other).args
        p NameError.new("nm", :sym).name
        p NameError.new("nm").name
        begin; "s".no_such; rescue NoMethodError => e; p e.receiver; p e.name; p e.args; end
        begin; nil.foo(1); rescue NoMethodError => n; p n.name; end
        begin; TypeError.new("m").name; rescue NoMethodError => e; puts e.class; end
        begin; Object.const_get(:Nope); rescue NameError => e; p e.name; p e.receiver; end
        begin; Nonexistent; rescue NameError => e; p e.name; end
        v = begin; throw :y; rescue UncaughtThrowError => e; e.tag; end; p v
        w = begin; throw :z, 7; rescue UncaughtThrowError => e; e.value; end; p w
        begin; raise "boom"; rescue => e; p e.detailed_message; end
        begin; raise RuntimeError; rescue => e; p e.detailed_message; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        ":sym\n42\n[1, 2]\n:meth\n\"msg\"\nnil\n:sym\nnil\n\"s\"\n:no_such\n[]\n:foo\n\
         NoMethodError\n:Nope\nObject\n:Nonexistent\n:y\n7\n\"boom (RuntimeError)\"\n\
         \"RuntimeError (RuntimeError)\"\n"
    );
}

#[test]
fn bad_manifests_are_loud_configuration_errors() {
    // Name/directory mismatch.
    let err = compile_packages(
        &[
            // Gem named "bbb" living in a directory named "aaa".
            ("packages/aaa/aaa.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"bbb\"\n  s.version = \"1.0.0\"\nend\n"),
            ("packages/aaa/lib/aaa.rb", "puts 1\n"),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(err.contains("doesn't match its directory name"), "unexpected error: {err}");

    // A gemspec that sets no name.
    let err = compile_packages(
        &[
            ("packages/aaa/aaa.gemspec", "Gem::Specification.new do |s|\n  s.version = \"1.0.0\"\nend\n"),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(err.contains("sets no `name`"), "unexpected error: {err}");

    // Default require_paths (["lib"]) pointing at a missing lib/.
    let err = compile_packages(
        &[
            ("packages/aaa/aaa.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"aaa\"\n  s.version = \"1.0.0\"\nend\n"),
            ("packages/aaa/aaa.rb", "puts 1\n"),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(err.contains("doesn't exist under"), "unexpected error: {err}");
}

#[test]
fn in_tree_base64_over_two_args_raises_argument_error_at_runtime() {
    // The in-tree Base64 ext checks arity at runtime (a rescuable
    // ArgumentError) via `arity!`.
    let result = run_ruby(
        r#"
        require "base64"
        begin
          Base64.encode64("a", "b")
        rescue ArgumentError
          puts "argerr"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "argerr\n");
}

#[test]
fn new_with_the_wrong_arity_raises_a_rescuable_runtime_error() {
    // Was a COMPILE-TIME panic, from `.new`'s own hand-rolled argument
    // binding. Now that `.new` binds through the same `emit_call_args_to`
    // every other call site uses, it inherits that machinery's posture,
    // which is real Ruby's: arity resolves at RUNTIME, the error is
    // rescuable, and a never-executed bad call compiles fine. Both lines
    // oracle-verified, message included.
    let result = run_ruby(
        r#"
        class Bag
          def initialize(a, b = 1); end
        end
        begin
          Bag.new(1, 2, 3)
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        def never_called
          Bag.new(1, 2, 3)
        end
        puts "compiled fine"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: wrong number of arguments (given 3, expected 1..2)\ncompiled fine\n"
    );
}

#[test]
fn dynamic_dispatch_wrong_arity_raises_a_rescuable_argument_error() {
    // `send` routes through `emit_dynamic_trampoline`, whose arity guard used
    // to `panic!` -- a process abort where Ruby raises a RESCUABLE
    // ArgumentError. Now it emits `return Err(raise_error("ArgumentError",
    // ...))`, so the program survives every mismatch and the final line
    // prints. All three message shapes -- fixed `N`, range `N..M`, and the
    // rest form `N+` -- are oracle-verified against ruby 4.0.5.
    let result = run_ruby(
        r##"
        class A
          def fixed(a) = a
          def rng(a, b = 2) = a + b
          def restp(a, *b) = a
        end
        o = A.new
        [[:fixed, []], [:fixed, [1, 2]], [:rng, []], [:rng, [1, 2, 3]], [:restp, []]].each do |m, args|
          begin
            o.send(m, *args)
          rescue ArgumentError => e
            puts "#{m}: #{e.message}"
          end
        end
        puts "survived"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "fixed: wrong number of arguments (given 0, expected 1)\n\
         fixed: wrong number of arguments (given 2, expected 1)\n\
         rng: wrong number of arguments (given 0, expected 1..2)\n\
         rng: wrong number of arguments (given 3, expected 1..2)\n\
         restp: wrong number of arguments (given 0, expected 1+)\n\
         survived\n"
    );
}

#[test]
fn value_trampoline_wrong_arity_raises_a_rescuable_argument_error() {
    // The `ValueMethodFn` trampoline (`emit_value_trampoline`) backs both a
    // user CLASS method reached dynamically and a reopened-builtin instance
    // method. Its arity guard shared the same `panic!` bug as the dynamic
    // trampoline; both now raise a rescuable ArgumentError. Oracle-verified.
    let result = run_ruby(
        r##"
        class A
          def self.cm(a, b) = a + b
        end
        class Integer
          def double(a); self * 2; end
        end
        k = A
        begin
          k.send(:cm, 1)
        rescue ArgumentError => e
          puts "cm: #{e.message}"
        end
        begin
          5.send(:double)
        rescue ArgumentError => e
          puts "double: #{e.message}"
        end
        puts "survived"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "cm: wrong number of arguments (given 1, expected 2)\n\
         double: wrong number of arguments (given 0, expected 1)\n\
         survived\n"
    );
}

#[test]
fn reopen_guards_mirror_rubys_type_errors() {
    // `TypeError: superclass mismatch for class Sub` in real Ruby.
    let err = spinelc::compile_to_rust(
        "class Base\nend\nclass Other\nend\nclass Sub < Base\nend\nclass Sub < Other\nend\n",
    )
    .unwrap_err();
    assert!(
        err.contains("superclass mismatch for class Sub"),
        "unexpected error: {err}"
    );

    // Restating the ORIGINAL superclass is allowed (real Ruby).
    let result = run_ruby(
        r#"
        class Base
        end

        class Sub < Base
        end

        class Sub < Base
          def ok
            "explicit matching superclass ok"
          end
        end

        puts Sub.new.ok
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "explicit matching superclass ok\n");

    // `TypeError: Foo is not a module` / `Bar is not a class` in real Ruby.
    let err = spinelc::compile_to_rust("class Foo\nend\nmodule Foo\nend\n").unwrap_err();
    assert!(err.contains("Foo is not a module"), "unexpected error: {err}");
    let err = spinelc::compile_to_rust("module Bar\nend\nclass Bar\nend\n").unwrap_err();
    assert!(err.contains("Bar is not a class"), "unexpected error: {err}");
}

#[test]
fn exception_classes_work_across_namespaces() {
    // Subclassing a nested error class from outside, raising with a
    // qualified path, and rescue-matching through the shared ancestor.
    let result = run_ruby(
        r#"
        module Errs
          class Base3 < StandardError
          end
        end

        class Deep < Errs::Base3
        end

        begin
          raise Deep, "boom"
        rescue Errs::Base3 => e
          puts "caught #{e.message}"
        end

        begin
          raise Errs::Base3, "direct"
        rescue StandardError => e
          puts "caught #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught boom\ncaught direct\n");
}

#[test]
fn dot_class_works_on_rescue_bindings_and_poly_receivers() {
    let result = run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          puts e.class
        end

        mixed = [1, "two", nil]
        mixed.each do |v|
          puts v.class
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "RuntimeError\nInteger\nString\nNilClass\n"
    );
}

#[test]
fn no_method_error_messages_name_the_real_class() {
    let result = run_ruby(
        r#"
        class Widget
        end

        w = [Widget.new].first
        begin
          w.nope
        rescue NoMethodError => e
          puts e.message
        end

        module Helper
        end

        begin
          Helper.new
        rescue NoMethodError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "undefined method 'nope' for an instance of Widget\nundefined method 'new' for module Helper\n"
    );
}

/// The table rows validate argument TYPES with CRuby's exact TypeError/
/// ArgumentError messages (previously an arg-type mismatch degraded to
/// NoMethodError) -- all real, rescuable exceptions now.
#[test]
fn builtin_arg_mismatches_raise_cruby_error_shapes() {
    let result = run_ruby(
        r##"
        a = ["a"].first
        begin
          a + 1
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        arr = [[1]].first
        begin
          arr[:x]
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        one = [1].first
        begin
          one < "a"
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "TypeError: no implicit conversion of Integer into String\n\
         TypeError: no implicit conversion of Symbol into Integer\n\
         ArgumentError: comparison of Integer with String failed\n"
    );
}

#[test]
fn rescue_matches_an_included_module() {
    let result = run_ruby(
        r#"
        module Alertable; end
        class AppError < StandardError
          include Alertable
        end
        begin
          raise AppError, "direct"
        rescue Alertable => e
          puts "alertable: #{e.message}"
        end
        "#,
    );
    assert_eq!(result.stdout, "alertable: direct\n");
}

#[test]
fn raise_with_a_bare_class_runs_the_classs_own_initialize() {
    // `raise E` must CONSTRUCT via `E.new` -- running defaults and `super`
    // -- rather than short-cutting the message to the class name. The
    // class-name default lives in the prelude's `Exception#to_s`.
    let result = run_ruby(
        r##"
        class E < StandardError
          def initialize(m = "def")
            super
          end
        end
        class F < StandardError
          def initialize(m = "dd")
            super("wrapped: #{m}")
          end
        end
        class G < StandardError; end

        begin; raise E; rescue => e; p e.message; end
        begin; raise E, "x"; rescue => e; p e.message; end
        begin; raise E.new; rescue => e; p e.message; end
        begin; raise F; rescue => e; p e.message; end
        begin; raise G; rescue => e; p e.message; end
        begin; raise StandardError; rescue => e; p e.message; end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"def\"\n\"x\"\n\"def\"\n\"wrapped: dd\"\n\"G\"\n\"StandardError\"\n"
    );
}

#[test]
fn an_exception_ivar_set_by_a_custom_initialize_survives_the_raise() {
    let result = run_ruby(
        r##"
        class Detailed < StandardError
          def initialize(field)
            @field = field
            super("invalid #{field}")
          end
          attr_reader :field
        end
        begin
          raise Detailed.new("email")
        rescue Detailed => e
          p [e.message, e.field]
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"invalid email\", \"email\"]\n");
}

#[test]
fn method_of_an_unknown_name_raises_name_error_at_construction() {
    let result = run_ruby(
        r#"
        begin
          "str".method(:definitely_not_defined)
        rescue NameError => e
          puts e.message
        end
        class WithPrivate
          private
          def secret = "shh"
        end
        p WithPrivate.new.method(:secret).call
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "undefined method 'definitely_not_defined' for class 'String'\n\"shh\"\n"
    );
}

#[test]
fn exception_backtrace_full_message_and_inspect() {
    // backtrace is an empty array (spinel does not track per-exception
    // backtraces); inspect renders "#<Class: msg>" (or the bare class name
    // when the message is empty); full_message is "Class: msg".
    let result = run_ruby(
        r#"
        begin
          raise ArgumentError, "bad"
        rescue => e
          p e.backtrace
          p e.inspect
          puts e.full_message
        end
        p StandardError.new("").inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[]\n\"#<ArgumentError: bad>\"\nArgumentError: bad\n\"StandardError\"\n"
    );
}

#[test]
fn dollar_bang_reads_the_exception_being_handled() {
    // `$!` is the current exception inside a rescue (block or modifier form),
    // nil outside one.
    let result = run_ruby(
        r#"
        p $!
        r = (Integer("x") rescue $!.class)
        p r
        begin
          raise "boom"
        rescue
          puts $!.message
        end
        p $!
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\nArgumentError\nboom\nnil\n");
}

#[test]
fn eval_non_string_argument_is_a_type_error() {
    let result = run_ruby(
        r#"
        begin
          eval(123)
        rescue TypeError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "no implicit conversion of Integer into String\n"
    );
}

#[test]
fn eval_syntax_error_is_catchable_at_runtime() {
    let result = run_ruby(
        r#"
        bad = "1 +"
        begin
          eval(bad)
        rescue SyntaxError
          puts "caught"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught\n");
}

#[test]
fn exception_cause_chains_from_active_rescue() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise "inner"
          rescue
            raise "outer"
          end
        rescue => e
          puts e.message
          puts e.cause.message
          puts e.cause.class
        end
        begin
          raise "solo"
        rescue => e
          p e.cause
        end
        begin
          begin
            Integer("x")
          rescue
            raise ArgumentError, "wrapped"
          end
        rescue => e
          puts e.cause.class
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "outer\ninner\nRuntimeError\nnil\nArgumentError\n"
    );
}

#[test]
fn return_from_proc_whose_home_is_gone_raises_localjumperror() {
    // Once the home method has unwound -- via exception OR normal return --
    // the Proc's `return` finds no live home and raises LocalJumpError rather
    // than leaking a Signal::Return.
    let result = run_ruby(
        r#"
        $escaped = nil
        def home_raises; $escaped = proc { return 99 }; raise "boom"; end
        begin; home_raises; rescue; end
        begin
          $escaped.call; puts "WRONG"
        rescue LocalJumpError => e
          puts "exc: #{e.message}"
        end

        class Deferred
          def arm; @job = proc { return :never }; self; end
          def fire; @job.call; end
        end
        begin
          Deferred.new.arm.fire; puts "WRONG"
        rescue LocalJumpError => e
          puts "norm: #{e.message}"
        end

        def collect; [proc { return 4 }]; end
        arr = collect
        begin
          arr[0].call; puts "WRONG"
        rescue LocalJumpError => e
          puts "arr: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "exc: unexpected return\nnorm: unexpected return\narr: unexpected return\n"
    );
}

#[test]
fn catch_throw_delivers_values_and_uncaught_throw_raises() {
    let result = run_ruby(
        r#"
        def thrower; throw :done, [1, 2]; end
        p catch(:done) { thrower }
        outer = Object.new; inner = Object.new
        p (catch(outer) { catch(inner) { throw outer, :to_outer }; :nr })
        begin
          throw :nope, 5; puts "WRONG"
        rescue UncaughtThrowError => e
          puts e.message
        end
        catch(:gone) { }
        begin
          throw :gone
        rescue UncaughtThrowError => e
          puts "gone: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n:to_outer\nuncaught throw :nope\ngone: uncaught throw :gone\n"
    );
}

#[test]
fn array_frozen_mutation_raises() {
    // Every mutating Array method raises FrozenError on a frozen receiver.
    let result = run_ruby(
        r#"
        a = [1, 2, 3].freeze
        def caught(a, name)
          yield
          "BUG #{name}"
        rescue FrozenError => e
          e.message
        end
        p caught(a, "reverse!") { a.reverse! }
        p caught(a, "sort!") { a.sort! }
        p caught(a, "delete") { a.delete(1) }
        p caught(a, "insert") { a.insert(0, 9) }
        p caught(a, "clear") { a.clear }
        p caught(a, "pop") { a.pop }
        p caught(a, "select!") { a.select! { true } }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    let line = "can't modify frozen Array: [1, 2, 3]";
    assert_eq!(result.stdout, format!("{line:?}\n").repeat(7));
}

#[test]
fn exception_equality_exception_method_and_message_coercion() {
    let result = run_ruby(
        r##"
        e = RuntimeError.new("m")
        e2 = RuntimeError.new("m")
        p e == e2
        p e == RuntimeError.new("other")
        p e == "not an exception"
        p e.eql?(e)
        p e.eql?(e2)
        p e.exception.equal?(e)
        p e.exception("n").message
        p e.exception("n").equal?(e)
        p RuntimeError.exception("z").message
        p RuntimeError.exception("z").class
        p RuntimeError.new(42).message
        p RuntimeError.new(:sym).message
        p ArgumentError.new([1, 2]).message
        p [true, false].include?(Exception.to_tty?)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\nfalse\ntrue\nfalse\ntrue\n\"n\"\nfalse\n\"z\"\nRuntimeError\n\"42\"\n\"sym\"\n\"[1, 2]\"\ntrue\n",
    );
}

#[test]
fn numeric_and_collection_error_protocol_edges() {
    // Float#% by zero raises ZeroDivisionError (not NaN); a negative first/last
    // count raises ArgumentError with the receiver-specific message.
    let result = run_ruby(
        r##"
        def t; yield; rescue => e; "#{e.class}: #{e.message}"; end
        p t { 5.0 % 0 }
        p t { 5.0 % 0.0 }
        p 5.0 % 2
        p(-5.5 % 2)
        p t { [1, 2, 3].first(-1) }
        p t { [1, 2, 3].last(-2) }
        p t { (1..3).first(-1) }
        p t { [1, 2, 3].cycle.first(-1) }
        p [1, 2, 3].first(2)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ZeroDivisionError: divided by 0\"\n\"ZeroDivisionError: divided by 0\"\n1.0\n0.5\n\"ArgumentError: negative array size\"\n\"ArgumentError: negative array size\"\n\"ArgumentError: negative array size (or size too big)\"\n\"ArgumentError: attempt to take negative size\"\n[1, 2]\n",
    );
}

#[test]
fn the_core_exception_tree_is_nameable_and_rescuable() {
    // The `Exception`-direct classes (uncaught by a bare `rescue`) and the
    // refined `StandardError`-branch classes all register with the right
    // superclass and can be raised/rescued by name.
    let result = run_ruby(
        r##"
        p SystemExit.superclass
        p Interrupt.superclass
        p NoMemoryError.superclass
        p NoMatchingPatternKeyError.superclass
        p Regexp::TimeoutError.superclass
        p IO::TimeoutError.superclass
        begin
          raise SecurityError, "denied"
        rescue Exception => e
          puts "#{e.class}: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Exception\nSignalException\nException\nNoMatchingPatternError\nRegexpError\nIOError\nSecurityError: denied\n",
    );
}

#[test]
fn super_with_no_definition_above_raises_at_runtime() {
    // Real Ruby has no definition-time check for `super`: it resolves against
    // the receiver's live ancestry at CALL time and raises NoMethodError only
    // if that walk comes up empty (vm_search_super_method / vm_eval.c). So a
    // `super` with nothing above it must compile, and the raise must be
    // rescuable -- rejecting it at compile time would kill this whole program.
    let result = run_ruby(
        r#"
        class Rec
          def as_json
            h = super
            h[:x] = 1
            h
          end
        end
        begin
          Rec.new.as_json
        rescue NoMethodError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "super: no superclass method 'as_json' for an instance of Rec\n",
    );
}

// --- Batch A6: eager-codegen panics that name a legitimate runtime error are
// deferred to a runtime raise, so an undefined-constant reference in a dead or
// rescued branch compiles cleanly (CRuby only raises `uninitialized constant`
// if the branch actually runs) instead of aborting the whole compile.

#[test]
fn rescue_naming_an_undefined_constant_compiles_and_never_fires_if_no_exception() {
    // The `rescue` clause's class expression is only evaluated while matching
    // an actually-raised exception. A clause that never fires must not fail
    // the compile just because its class isn't defined.
    let result = run_ruby(
        r##"
        begin
          1 + 1
        rescue NeverDefined
          puts "caught"
        end
        puts "ok"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok\n");
}

#[test]
fn rescue_naming_an_undefined_constant_raises_name_error_when_matched() {
    // When the body DOES raise, evaluating the undefined rescue constant is a
    // runtime NameError (which replaces the original exception) -- exactly
    // CRuby's behavior.
    let result = run_ruby(
        r##"
        begin
          begin
            raise "boom"
          rescue NeverDefined
            puts "caught"
          end
        rescue => e
          puts "#{e.class}: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "NameError: uninitialized constant NeverDefined\n");
}

#[test]
fn a_pattern_naming_an_undefined_constant_raises_name_error_when_tried() {
    let result = run_ruby(
        r##"
        x = 5
        r = begin
          case x
          in NopeClass then "a"
          else "b"
          end
        rescue => e
          "#{e.class}: #{e.message}"
        end
        puts r
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "NameError: uninitialized constant NopeClass\n");
}

#[test]
fn a_scoped_const_write_to_an_undefined_scope_raises_before_the_rhs_runs() {
    // CRuby resolves the scope BEFORE evaluating the value, so the RHS side
    // effect never runs.
    let result = run_ruby(
        r##"
        r = begin
          Nope::X = (puts "rhs-ran"; 5)
          "assigned"
        rescue => e
          "#{e.class}: #{e.message}"
        end
        puts r
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "NameError: uninitialized constant Nope\n");
}

#[test]
fn raising_an_undefined_constant_with_a_message_raises_name_error() {
    // `raise UndefinedConst, "msg"` -- CRuby evaluates the constant (and
    // raises NameError) before the message is ever consulted.
    let result = run_ruby(
        r##"
        r = begin
          raise Nope, "msg"
        rescue => e
          "#{e.class}: #{e.message}"
        end
        puts r
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "NameError: uninitialized constant Nope\n");
}

#[test]
fn singleton_method_yield_with_no_block_raises_rescuable_local_jump_error() {
    // A `yield` reached with no block is a RESCUABLE LocalJumpError, not a
    // process abort -- true for a singleton method now that its block is
    // threaded (Batch G).
    let result = run_ruby(
        r##"
        obj = Object.new
        def obj.needs_block
          yield
        end
        begin
          obj.needs_block
        rescue LocalJumpError => e
          puts "caught #{e.class}: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught LocalJumpError: no block given (yield)\n");
}

#[test]
fn an_ordinary_method_yield_with_no_block_raises_rescuable_local_jump_error() {
    // The same LocalJumpError path an ordinary method has always needed -- it
    // used to abort the process with a Rust panic instead of raising.
    let result = run_ruby(
        r##"
        def m
          yield
        end
        begin
          m
        rescue LocalJumpError => e
          puts "caught #{e.class}"
        end
        puts m { "with-block" }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught LocalJumpError\nwith-block\n");
}
