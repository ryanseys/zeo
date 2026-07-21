use crate::support::run_ruby;

#[test]
fn dynamic_send_with_a_non_literal_target() {
    let result = run_ruby(
        r#"
        class Greeter
          define_method(:hello) do
            puts :hi
          end
        end

        name = :hello
        Greeter.new.send(name)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\n");
}

#[test]
fn eval_of_a_literal_string_runs_inline() {
    let result = run_ruby(r#"puts eval("1 + 2")"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn eval_shares_the_enclosing_local_scope() {
    let result = run_ruby(
        r#"
        x = 1
        eval("x = x + 1")
        puts x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn eval_of_a_non_literal_argument_runs_in_the_vm() {
    // A non-literal source is no longer rejected at compile time; it runs
    // through the eval VM. (`y` is interpolated INTO the source string, not
    // referenced inside the eval.)
    let result = run_ruby("y = 40\nputs eval(\"#{y} + 2\")\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

/// The `needs_eval_vm` verdict drives `build::Runtime` selection: `true` links
/// the prism-backed runtime, `false` keeps the binary lean. This asserts the
/// decision itself (not just that programs run), because a false negative would
/// ship a lean binary whose `eval` is a `NotImplementedError` stub, and a false
/// positive needlessly drags prism into an eval-free binary.
#[test]
fn needs_eval_vm_selects_the_runtime_variant() {
    let needs = |src: &str| {
        zeo::compile_to_rust_with(src, &Default::default())
            .expect("compiles")
            .needs_eval_vm
    };

    // No eval anywhere -> lean.
    assert!(!needs("puts 1"));
    // An ACCEPTED literal eval is spliced inline (`HirNode::Eval`) -> lean.
    assert!(!needs(r#"puts eval("1 + 2")"#));
    // A block-form `instance_eval` runs a real block, never the VM -> lean.
    assert!(!needs("o = Object.new\no.instance_eval { 1 + 2 }\n"));

    // A dynamic (non-literal) eval reaches the runtime VM -> needs it.
    assert!(needs("s = \"1 + 2\"\neval(s)\n"));
    // A literal the inline path can't express (top-level class) falls through
    // to the runtime VM -> needs it.
    assert!(needs(r#"eval("class Foo; end")"#));
    // A string-form `instance_eval` reaches the VM -> needs it.
    assert!(needs("o = Object.new\no.instance_eval(\"@x = 1\")\n"));
    // A string-form `class_eval`/`module_eval` reaches it the same way -- the
    // verdict already said so, but the runtime row used to ignore its argument
    // and report "tried to create Proc object without a block" instead.
    assert!(needs("class Foo; end\nFoo.class_eval(\"1 + 2\")\n"));
    assert!(needs("module M; end\nM.module_eval(\"1 + 2\")\n"));
    // The BLOCK form of either still runs a real block -> lean.
    assert!(!needs("class Foo; end\nFoo.class_eval { 1 + 2 }\n"));
}

#[test]
fn dynamic_send_carries_a_block_through_to_a_yield_using_method() {
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        total = 0
        Collector.new.send(:each_num, 1, 2, 3) { |n| total += n }
        puts total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn match_predicate_bindings_escape_to_the_enclosing_scope() {
    let result = run_ruby(
        r#"
        arr = [1, "hi"]
        arr in [Integer => a, String => b]
        puts a
        puts b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\nhi\n");
}

#[test]
fn named_capture_auto_binding_assigns_a_local_per_group() {
    // `/(?<a>..)/ =~ str` assigns each named group to a local of that name.
    // Only with the literal on the LEFT -- `str =~ /(?<a>.)/` binds nothing,
    // which is Ruby's own asymmetry (the parser can only declare the locals
    // when it can see the names), not an approximation. Oracle-verified,
    // including that a failed match leaves each name nil.
    let result = run_ruby(
        r#"
        if /(?<first>\w+) (?<last>\w+)/ =~ "John Smith"
          puts first
          puts last
        end
        p(/(?<n>\d+)/ =~ "abc123")
        p n
        /(?<z>\d+)/ =~ "none"
        p z
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "John\nSmith\n3\n\"123\"\nnil\n");
}

#[test]
fn dup_dispatches_dynamically_through_send() {
    let result = run_ruby(
        r#"
        puts [1, 2].send(:dup).length
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

/// `respond_to?` walks the real MRO now: Enumerable names answer true on
/// arrays, Comparable names on strings, and Kernel privates stay invisible.
#[test]
fn respond_to_walks_the_mro() {
    let result = run_ruby(
        r#"
        puts [1, 2].respond_to?(:map)
        puts "s".respond_to?(:between?)
        puts 5.respond_to?(:puts)
        puts 5.respond_to?(:itself)
        puts "s".respond_to?(:nope)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\ntrue\nfalse\n");
}

#[test]
fn universal_reflection_and_to_set() {
    let result = run_ruby(
        r#"
        class Point
          def initialize(x, y)
            @x = x
            @y = y
          end
        end
        pt = Point.new(3, 4)
        p pt.instance_variables
        p pt.instance_variable_set(:@x, 99)
        p pt.instance_variable_get(:@x)
        p pt.instance_variable_defined?(:@y)
        p pt.instance_variable_defined?(:@z)
        p 42.instance_variables
        p 42.singleton_methods
        p 42.respond_to?(:instance_variable_get)
        p 42.send(:instance_variables)
        p [1, 2, 2, 3].to_set
        p (1..3).to_set
        p({ a: 1, b: 2 }.to_set.size)
        p(/x/.timeout)
        p Regexp.timeout
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:@x, :@y]\n99\n99\ntrue\nfalse\n[]\n[]\ntrue\n[]\nSet[1, 2, 3]\nSet[1, 2, 3]\n2\nnil\nnil\n"
    );
}

// ---------------------------------------------------------------------------
// Runtime string `eval` / `instance_eval` (#97 stage 2 -- the eval VM).
//
// Every source below is held in a VARIABLE (or built with `.dup`/`+`), so it is
// NOT a string literal and therefore runs through the runtime eval VM (a
// tree-walking interpreter over prism), not the compile-time inline path a
// string literal takes. That is the surface these tests are here to cover.
// ---------------------------------------------------------------------------

#[test]
fn eval_dynamic_arithmetic_honours_precedence() {
    let result = run_ruby(r#"code = "1 + 2 * 3"; puts eval(code)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn eval_dynamic_method_call_on_evaluated_receiver() {
    let result = run_ruby(r#"src = "[3, 1, 2].sort.inspect"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2, 3]\n");
}

#[test]
fn eval_string_interpolation_inside_source() {
    let result = run_ruby(r#"code = 'n = 6; "n=#{n * n}"'; puts eval(code)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "n=36\n");
}

#[test]
fn eval_source_assembled_at_runtime() {
    let result = run_ruby(r#"op = "-"; puts eval("10 #{op} 3")"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn eval_control_flow_yields_last_expression() {
    let result = run_ruby(r#"src = "if 1 < 2 then :yes else :no end"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "yes\n");
}

#[test]
fn eval_short_circuit_returns_the_operand() {
    let result = run_ruby(r#"src = "nil || 'fallback'"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fallback\n");
}

#[test]
fn eval_collections_arrays_hashes_ranges() {
    let result = run_ruby(
        r#"
        puts eval("[1, 2, 3, 4].length".dup)
        puts eval("{ a: 1, b: 2 }.length".dup)
        puts eval("(1..5).to_a.inspect".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4\n2\n[1, 2, 3, 4, 5]\n");
}

#[test]
fn eval_value_is_usable_in_the_surrounding_expression() {
    let result = run_ruby(r#"a = "6"; b = "7"; puts(eval(a) * eval(b))"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn instance_eval_string_rebinds_self_to_a_literal_receiver() {
    let result = run_ruby(r#"src = "upcase.reverse"; puts "hello".instance_eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "OLLEH\n");
}

#[test]
fn module_const_get_resolves_or_raises_name_error() {
    let result = run_ruby(
        r#"
        class Foo; BAR = 42; end
        p Foo.const_get(:BAR)
        p Foo.const_get("BAR")
        p Object.const_get(:Foo)
        p((Object.const_get(:MissingXYZ) rescue $!.class))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n42\nFoo\nNameError\n");
}
