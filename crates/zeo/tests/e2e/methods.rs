use crate::support::{run_ruby};

#[test]
fn inheritance_super_chain() {
    let result = run_ruby(
        r#"
        class Animal
          def speak
            puts :generic
          end
        end

        class Dog < Animal
          def speak
            super
            puts :woof
          end
        end

        Dog.new.speak
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "generic\nwoof\n");
}

#[test]
fn method_missing_fallback_on_a_total_miss() {
    let result = run_ruby(
        r#"
        class Greeter
          def method_missing(name)
            puts :missing
          end
        end

        Greeter.new.send(:nonexistent)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "missing\n");
}

#[test]
fn class_shift_self_at_top_level_defines_singleton_methods_on_main() {
    // `class << self` at the top level reopens `main`'s singleton (Batch G):
    // it desugars to `self.define_singleton_method(...)`, `self` being `main`,
    // so each inner `def` installs on `main` and is callable via implicit self.
    let result = run_ruby(
        r#"
        class << self
          def shout; "MAIN"; end
          def echo(x); "<#{x}>"; end
        end
        puts shout
        puts echo("hi")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "MAIN\n<hi>\n");
}

#[test]
fn rest_and_post_params_bind_a_real_array() {
    let result = run_ruby(
        r##"
        class Collector
          def count(*nums)
            nums.length
          end
          def between(a, *mid, z)
            "#{a}-#{mid.length}-#{z}"
          end
        end
        c = Collector.new
        puts c.count(1, 2, 3)
        puts c.count
        puts c.between(1, 2, 3, 9)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n0\n1-2-9\n");
}

#[test]
fn keyword_params_bind_by_name_regardless_of_call_site_order() {
    let result = run_ruby(
        r#"
        class Kw
          def greet(x:, y: 10)
            x + y
          end
          def opts(**rest)
            rest.length
          end
        end
        k = Kw.new
        puts k.greet(x: 1)
        puts k.greet(x: 1, y: 2)
        puts k.greet(y: 3, x: 4)
        puts k.opts(a: 1, b: 2, c: 3)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "11\n3\n7\n3\n");
}

#[test]
fn attr_accessor_reader_and_writer_generate_real_methods() {
    let result = run_ruby(
        r#"
        class Box
          attr_accessor :value
          attr_reader :ro
          def initialize
            @value = 1
            @ro = 7
          end
        end
        b = Box.new
        puts b.value
        b.value = 99
        puts b.value
        puts b.ro
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n99\n7\n");
}

#[test]
fn visibility_keywords_switch_the_default_for_subsequent_defs() {
    // A bare `private`/`public` switches the DEFAULT visibility for every
    // subsequent `def` in the class body (see
    // `parse::lower_class_body_statement`'s docs) -- this test only checks
    // that a PUBLIC method compiles/runs normally after a `private` section;
    // see the dedicated visibility-enforcement tests below for the actual
    // private/protected/public_send rejection cases.
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
          public
          def x
            @x
          end
        end
        puts Box.new.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn endless_method_definition() {
    let result = run_ruby(
        r#"
        class Calc
          def double(x) = x * 2
        end
        puts Calc.new.double(21)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn arithmetic_on_a_plain_method_parameter_works() {
    // Method params are always statically `Poly` (zeo never infers a
    // param's type from call sites) -- this exercises the runtime-checked
    // fallback in `codegen::call::dispatch`, not just literal/local `Int`
    // operands.
    let result = run_ruby(
        r#"
        class Adder
          def add(a, b)
            a + b
          end
        end
        puts Adder.new.add(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn a_named_block_parameter_can_be_called_explicitly() {
    // A `&block` parameter is real syntax now (Phase 6 lifted the Phase-5
    // rejection this test used to check for).
    let result = run_ruby(
        "class Foo\n  def bar(&blk)\n    blk.call(5)\n  end\nend\nputs Foo.new.bar { |x| x * 2 }\n",
    );
    assert_eq!(result.stdout, "10\n");
}

#[test]
fn forwarding_params_forward_positionals_keywords_and_block() {
    // `...` is real now (G3): it desugars to internal `*__fwd_rest,
    // **__fwd_kw, &__fwd_blk` params referenced by the call-site `...`.
    let result = run_ruby(
        r##"
        def target(a, b, mode: "m")
          r = yield if block_given?
          "#{a}/#{b}/#{mode}/#{r.inspect}"
        end
        def fwd(...)
          target(...)
        end
        puts fwd(1, 2, mode: "z") { "blk" }
        "##,
    );
    assert_eq!(result.stdout, "1/2/z/\"blk\"\n");
}

#[test]
fn numbered_params_work_through_a_real_escaping_proc() {
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        Collector.new.each_num(5, 6) { puts _1 * 2 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n12\n");
}

#[test]
fn a_block_with_more_params_than_yielded_values_nil_fills_leniently() {
    // `puts` on `nil` prints an empty line -- real Ruby's own behavior, and
    // avoids `Object#inspect` (a separate, unrelated, unimplemented method).
    let result = run_ruby(
        r#"
        class Collector
          def each_one(a)
            yield a
          end
        end
        Collector.new.each_one(1) { |a, b| puts a; puts b }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n\n");
}

#[test]
fn keyword_params_on_a_block_bind_from_a_trailing_yielded_hash() {
    let result = run_ruby(
        r#"
        class Collector
          def emit
            yield x: 1, y: 2
          end
        end
        Collector.new.emit { |x:, y: 10| puts x + y }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn a_named_block_param_can_be_forwarded_to_another_call() {
    // `c` is a `New`-assigned LOCAL inside `go`, not a parameter or an
    // implicit-self call target -- see the self-capture test's comment for
    // why (both are separate, pre-existing, unrelated gaps).
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        class Relay
          def go(a, b, &blk)
            c = Collector.new
            c.each_num(a, b, &blk)
          end
        end
        total = 0
        Relay.new.go(3, 4) { |n| total += n }
        puts total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn define_singleton_method_on_object_and_class() {
    // #97 F2: a per-object singleton (only that object responds) and a
    // class-level singleton method (a class method).
    let result = run_ruby(
        r#"
        class Widget; end
        a = Widget.new
        a.define_singleton_method(:special) { "just me" }
        puts a.special
        puts Widget.new.respond_to?(:special)
        Widget.define_singleton_method(:factory) { "built" }
        puts Widget.factory
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "just me\nfalse\nbuilt\n");
}

#[test]
fn def_on_an_object_defines_a_per_object_singleton() {
    // #97 F3: `def obj.name` and `class << obj` install per-object singleton
    // methods (identity-keyed overlay), reaching `@ivar`/`self`/params and
    // answering `respond_to?` only on that object.
    let result = run_ruby(
        r##"
        obj = Object.new
        obj.instance_variable_set(:@n, 10)
        def obj.double
          @n * 2
        end
        class << obj
          def plus(k)
            @n + k
          end
        end
        puts obj.double
        puts obj.plus(5)
        puts obj.respond_to?(:double)
        puts obj.respond_to?(:plus)
        puts Object.new.respond_to?(:double)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n15\ntrue\ntrue\nfalse\n");
}

#[test]
fn super_from_an_override_reaches_an_included_module_method() {
    let result = run_ruby(
        r#"
        module Tagged
          def tag
            "base(#{super})"
          end
        end
        class Widget
          include Tagged
          def tag
            "widget"
          end
        end
        puts Widget.new.tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "widget\n");
}

#[test]
fn a_module_function_is_callable_via_its_own_def_self() {
    let result = run_ruby(
        r#"
        module Utility
          def self.triple(x)
            x * 3
          end
        end
        puts Utility.triple(4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "12\n");
}

#[test]
fn a_three_level_super_chain_crosses_a_module_boundary() {
    // `Parent#greet` calls `super`, reaching `Middle#greet` (an included
    // module), which itself calls `super`, reaching `GrandParent#greet` --
    // a chain of 3, verifying `emit_super_inline`'s ancestors-based search
    // composes correctly across more than one hop and a mixed
    // class/module boundary.
    let result = run_ruby(
        r#"
        class GrandParent
          def greet
            "grandparent"
          end
        end
        module Middle
          def greet
            "middle(#{super})"
          end
        end
        class Parent < GrandParent
          include Middle
          def greet
            "parent(#{super})"
          end
        end
        puts Parent.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "parent(middle(grandparent))\n");
}

#[test]
fn struct_custom_initialize_supers_into_the_member_setter() {
    // A custom `initialize` in the block calls `super` (bare or explicit,
    // positional) into the synthesized member-setter, reached via a two-level
    // base/leaf hierarchy -- not the old "no initialize above" panic.
    let result = run_ruby(
        r#"
        Trip = Struct.new(:x, :y, :z) do
          def initialize(x, y)
            super
          end
        end
        p Trip.new(1, 2).to_a
        V = Struct.new(:a, :b, :c) do
          def initialize(a, b)
            super(a, b, a + b)
          end
        end
        p V.new(3, 4).to_a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2, nil]\n[3, 4, 7]\n");
}

#[test]
fn clone_runs_user_initialize_copy_hook_with_super() {
    // clone/dup dispatch the user's initialize_copy (deep-copying a
    // shared member), and its bare `super` resolves Object's default no-op
    // hook instead of panicking.
    let result = run_ruby(
        r#"
        class Board
          def initialize
            @table = [[1], [2]]
          end
          def initialize_copy(orig)
            super
            @table = @table.clone
          end
          def push_row
            @table.push([9])
          end
          def size
            @table.length
          end
        end
        b = Board.new
        c = b.clone
        c.push_row
        puts c.size
        puts b.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n2\n");
}

#[test]
fn data_custom_initialize_supers_with_keyword_args() {
    // an explicit keyword `super(x: .., y: ..)` binds the parent's
    // keyword params by name; omitting a required one raises CRuby's
    // `missing keyword` ArgumentError.
    let result = run_ruby(
        r#"
        Point = Data.define(:x, :y) do
          def initialize(x:, y:)
            super(x: x * 100, y: y + 1)
          end
        end
        def mk(a, b); Point.new(x: a, y: b); end
        p mk(5, 2)
        Pair = Data.define(:m, :n) do
          def initialize(m:, n:)
            super(m: m)
          end
        end
        begin
          Pair.new(m: 1, n: 2)
        rescue ArgumentError => e
          puts "err: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "#<data Point x=500, y=3>\nerr: missing keyword: :n\n");
}

/// A `Hash` subclass with a `super` in `initialize` (D3): `super(0)` seeds the
/// default value on the payload, and inherited `[]`/`[]=`/`size` work.
#[test]
fn value_subclass_of_hash_with_super_initialize() {
    let result = run_ruby(
        r#"
        class Counter < Hash
          def initialize
            super(0)
          end
          def bump(k); self[k] += 1; end
        end
        c = Counter.new
        c.bump(:a); c.bump(:a); c.bump(:b)
        puts c[:a]
        puts c[:b]
        puts c[:missing]
        p c
        puts c.class
        puts c.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n1\n0\n{a: 2, b: 1}\nCounter\n2\n");
}

/// A user subclass of an IMMEDIATE builtin (D3): the DEFINITION is allowed --
/// `superclass`/`ancestors`/`is_a?` resolve -- but there are no instances, so
/// `MyInt.new` raises CRuby's exact `NoMethodError`.
#[test]
fn immediate_builtin_subclass_defines_but_cannot_instantiate() {
    let result = run_ruby(
        r##"
        class MyInt < Integer; end
        puts MyInt.superclass
        puts MyInt.ancestors.include?(Integer)
        puts MyInt.ancestors.include?(Numeric)
        begin
          MyInt.new
        rescue => e
          puts "#{e.class}: #{e.message}"
        end
        class MySym < Symbol; end
        begin; MySym.new; rescue => e; puts e.class; end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Integer\ntrue\ntrue\nNoMethodError: undefined method 'new' for class MyInt\nNoMethodError\n"
    );
}

/// A builtin reopen may RESTATE the class's real superclass (`class String <
/// Object`); the methods attach exactly as a clauseless reopen (D3).
#[test]
fn builtin_reopen_with_matching_superclass_clause() {
    let result = run_ruby(
        r#"
        class String < Object
          def shout
            upcase + "!"
          end
        end
        puts "hi".shout
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "HI!\n");
}

#[test]
fn nested_escaping_block_captures_an_inlined_times_param() {
    // An escaping closure capturing the param of an INLINED `.times` block
    // (Batch H1). The `.times` body shares the enclosing Rust scope, so the
    // param is cell-wrapped per iteration -- fresh each turn, matching Ruby --
    // and the nested closure `Arc::clone`s it, exactly like a real block
    // param. Previously a clean compile-error scope-cut.
    let result = run_ruby(
        r#"
        store = []
        [1, 2].each do |a|
          store << ->() { a }
          2.times do |b|
            store << ->() { a + b }
          end
        end
        store.each { |p| puts p.call }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    // a=1: {1}, {1+0}, {1+1}; a=2: {2}, {2+0}, {2+1}
    assert_eq!(result.stdout, "1\n1\n2\n2\n2\n3\n");
}

// Correctness-fix regression tests below -- each reproduces one bug a
// codebase-wide gap audit found (see docs/PORTING_ANALYSIS.md).

#[test]
fn super_with_explicit_args_binds_the_parents_own_param_names() {
    // Before this fix, `emit_super_inline` spliced the parent's body without
    // ever binding its parameter names -- this only "worked" when parent/
    // child happened to share names. Here they deliberately DON'T (`name`
    // vs. `name, breed`), so this only passes once `super(name)` actually
    // binds Animal's own `name` parameter fresh.
    let result = run_ruby(
        r#"
        class Animal
          def initialize(name)
            @name = name
          end
          def name
            @name
          end
        end

        class Dog < Animal
          def initialize(name, breed)
            super(name)
            @breed = breed
          end
          def breed
            @breed
          end
        end

        d = Dog.new("Rex", "Lab")
        puts d.name
        puts d.breed
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Rex\nLab\n");
}

#[test]
fn bare_super_forwards_the_current_methods_own_params() {
    // Bare `super` (no parens) forwards the CURRENT method's own already-
    // bound parameter values positionally -- also unbound before this fix
    // (same root cause as the explicit-args case above).
    let result = run_ruby(
        r#"
        class Animal
          def initialize(name)
            @name = name
          end
          def name
            @name
          end
        end

        class Dog < Animal
          def initialize(name)
            super
            @greeting = "woof from #{name}"
          end
          def greeting
            @greeting
          end
        end

        d = Dog.new("Rex")
        puts d.name
        puts d.greeting
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Rex\nwoof from Rex\n");
}

#[test]
fn send_with_a_non_literal_target_binds_keyword_args() {
    // The G2 trailing-kwargs-hash convention: `send`'s truly-dynamic
    // fallback (non-literal target name) carries kwargs as one trailing
    // Hash; the callee's trampoline pops and binds it.
    let result = run_ruby(
        r#"
        class Foo
          def bar(x:)
            x
          end
        end
        name = :bar
        puts Foo.new.send(name, x: 41) + 1
        "#,
    );
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn class_methods_take_the_full_param_shapes() {
    let result = run_ruby(
        r#"
        class P
          def self.m(a, b = 2, *rest, k: 9, **kw, &blk)
            [a, b, rest, k, kw, blk ? blk.call : nil]
          end
        end
        p P.m(1)
        p P.m(1, 3, 4, 5, k: 0, z: 1) { "blk" }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, [], 9, {}, nil]\n[1, 3, [4, 5], 0, {z: 1}, \"blk\"]\n"
    );
}

#[test]
fn private_def_idiom_and_retroactive_private_by_name() {
    let result = run_ruby(
        r#"
        class A
          def pub_a
            priv_a
          end

          private def priv_a
            "priv_a"
          end
        end

        class B
          def pub_b
            priv_b
          end

          def priv_b
            "priv_b"
          end

          private :priv_b
        end

        puts A.new.pub_a
        puts B.new.pub_b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "priv_a\npriv_b\n");
}

#[test]
fn send_bypasses_visibility_entirely() {
    let result = run_ruby(
        r#"
        class Box
          private

          def secret
            "shh"
          end
        end

        puts Box.new.send(:secret)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "shh\n");
}

#[test]
fn public_send_still_enforces_visibility_unlike_send() {
    // Same intent as when this asserted a compile-time panic, now asserting
    // the CORRECT mechanism: real Ruby resolves visibility at call time
    // (`rb_method_call_status`, vm_eval.c:837) and raises a rescuable
    // NoMethodError. Rejecting it during codegen was wrong -- it killed any
    // program that merely mentions such a call, even in a rescued branch --
    // so the program must now compile and the raise must be catchable, while
    // `send` stays visibility-blind.
    let result = run_ruby(
        r#"
        class Box
          private

          def secret
            "shh"
          end
        end

        begin
          puts Box.new.public_send(:secret)
        rescue NoMethodError => e
          puts e.message
        end
        puts Box.new.send(:secret)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "private method 'secret' called for an instance of Box\nshh\n",
    );
}

#[test]
fn compound_assignment_on_an_attribute_evaluates_the_receiver_exactly_once() {
    let result = run_ruby(
        r#"
        class Box
          attr_accessor :n
          def initialize
            @n = 0
          end
        end
        class Tracker
          attr_reader :calls
          def initialize(box)
            @box = box
            @calls = 0
          end
          def get
            @calls += 1
            @box
          end
        end

        b = Box.new
        t = Tracker.new(b)
        t.get.n += 5
        puts b.n
        puts t.calls
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n1\n");
}

#[test]
fn or_assign_on_an_attribute_short_circuits_without_calling_the_setter() {
    let result = run_ruby(
        r#"
        class Box
          attr_accessor :n
        end
        b = Box.new
        b.n = 5
        b.n ||= 99
        puts b.n
        b.n = nil
        b.n ||= 42
        puts b.n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n42\n");
}

#[test]
fn kwargs_double_splat_at_a_call_site_binds() {
    // `**h` merges into the G2 trailing-kwargs Hash (literal pairs first,
    // splat entries after, same-key replacement -- `Hash#merge`'s rule).
    let result = run_ruby(
        r##"
        class Greeter
          def f(x:, y: 0)
            "#{x}/#{y}"
          end
        end
        h = {x: 1}
        puts Greeter.new.f(**h)
        puts Greeter.new.f(y: 5, **{x: 2, y: 9})
        "##,
    );
    assert_eq!(result.stdout, "1/0\n2/9\n");
}

#[test]
fn method_arity_parameters_and_unbound_bind() {
    let result = run_ruby(
        r#"
        class C
          def a(x, y); x + y; end
          def b(x, y=1); end
          def d(x, y:, z: 2); end
          def f(x, *r, z, k:, **o, &blk); end
          def g; end
        end
        o = C.new
        puts o.method(:a).arity                 # 2
        puts o.method(:a).parameters.inspect     # [[:req, :x], [:req, :y]]
        puts o.method(:b).arity                 # -2
        puts o.method(:d).arity                 # 2
        puts o.method(:d).parameters.inspect     # [[:req,:x],[:keyreq,:y],[:key,:z]]
        puts o.method(:f).arity                 # -4
        puts o.method(:g).arity                 # 0
        um = C.instance_method(:a)
        puts um.class                           # UnboundMethod
        puts um.name                            # a
        puts um.arity                           # 2
        puts um.bind(o).call(2, 3)              # 5
        puts um.bind_call(o, 4, 5)              # 9
        puts o.method(:a).unbind.class          # UnboundMethod
        begin
          um.bind(42)
        rescue TypeError
          puts "typeerror"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2\n[[:req, :x], [:req, :y]]\n-2\n2\n[[:req, :x], [:keyreq, :y], [:key, :z]]\n-4\n0\nUnboundMethod\na\n2\n5\n9\nUnboundMethod\ntypeerror\n"
    );
}

#[test]
fn methods_and_instance_methods_reflect_names_with_visibility_and_inheritance() {
    let result = run_ruby(
        r#"
        module M
          def helper; end
        end
        class Base
          def pub; end
          private
          def priv; end
        end
        class Sub < Base
          include M
          def own; end
          def pub; end          # override
          def self.factory; end
        end
        puts Sub.instance_methods(false).sort.inspect   # [:own, :pub]
        puts Sub.instance_methods.include?(:helper)      # true (module)
        puts Sub.instance_methods.include?(:pub)         # true
        puts Sub.instance_methods.include?(:priv)        # false (private)
        puts Base.private_instance_methods(false).inspect # [:priv]
        puts M.instance_methods.inspect                  # [:helper]
        puts Sub.singleton_methods.inspect               # [:factory]
        o = Sub.new
        puts o.methods.include?(:own)                    # true
        puts o.methods.include?(:factory)                # false
        puts o.private_methods.include?(:priv)           # true
        puts "s".methods.include?(:upcase)               # true
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:own, :pub]\ntrue\ntrue\nfalse\n[:priv]\n[:helper]\n[:factory]\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn is_a_against_a_poly_typed_method_parameter() {
    // A method PARAMETER is always `TyKind::Poly` (zeo never infers a
    // param's type from its call sites) -- this exercises the runtime
    // `zeo_rt::is_a` fallback (via the new universal `RubyValue::class_id`)
    // rather than the static ancestors-constant-fold path.
    let result = run_ruby(
        r#"
        class Checker
          def check(v)
            v.is_a?(Integer)
          end
        end
        c = Checker.new
        puts c.check(5)
        puts c.check("hi")
        puts c.check([1, 2])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\nfalse\n");
}

#[test]
fn is_a_for_nil_true_and_false_against_their_own_singleton_classes() {
    let result = run_ruby(
        r#"
        puts nil.is_a?(NilClass)
        puts true.is_a?(TrueClass)
        puts false.is_a?(FalseClass)
        puts true.is_a?(Object)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\ntrue\n");
}

#[test]
fn reopening_object_defines_methods_reachable_everywhere() {
    // An Object reopen is top-level `def` by another name: its methods
    // land on arena slot 0, materialize into every user class, and
    // dispatch on the runtime `main` object at top-level call sites.
    let result = run_ruby(
        r#"
        class Object
          def double(x)
            x * 2
          end
        end
        puts double(21)
        class Widget
          def go
            double(4)
          end
        end
        puts Widget.new.go
        "#,
    );
    assert_eq!(result.stdout, "42\n8\n");
    assert_eq!(result.stderr, "");
}

#[test]
fn undef_removes_a_name_including_an_inherited_one() {
    // `undef` works on a name this class only INHERITS, which is why it
    // can't be "delete the local def" -- there is none. It stays live on
    // the ancestor that defined it, and `respond_to?` must agree.
    let result = run_ruby(
        r#"
        class B
          def inherited_m; "from B"; end
          def own; "own"; end
        end
        class C < B
          def local_m; "local"; end
          undef local_m
          undef inherited_m
        end
        begin; C.new.local_m; rescue NoMethodError; puts "local: NoMethodError"; end
        begin; C.new.inherited_m; rescue NoMethodError; puts "inherited: NoMethodError"; end
        p B.new.inherited_m
        p C.new.own
        p C.new.respond_to?(:inherited_m)
        class D
          def a; 1; end
          def b; 2; end
          undef a, b
        end
        begin; D.new.a; rescue NoMethodError; puts "D#a gone"; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "local: NoMethodError\ninherited: NoMethodError\n\"from B\"\n\"own\"\nfalse\nD#a gone\n"
    );
}

// --- Phase 12.8: alias / class << self ---------------------------------
//
// `alias` is resolved entirely at LOWERING time (`parse::lower_class_body_statement`):
// the aliased name's already-lowered `DefMethod` (params/body/visibility)
// is cloned under the new name, so no new analyze/codegen machinery exists
// at all -- it's indistinguishable from having written the method body
// twice under two names. `class << self` similarly desugars its nested
// `def`s to ordinary `is_class_method: true` `DefMethod`s (reusing
// `ClassInfo::class_methods` materialization Phase 12.6/Part 6 already
// built). Every test here was run against real `ruby` first, per this
// project's established convention.

#[test]
fn alias_creates_a_second_callable_name_for_the_same_method() {
    let result = run_ruby(
        r#"
        class Greeter
          def hello
            "hi"
          end
          alias hola hello
        end
        g = Greeter.new
        puts g.hello
        puts g.hola
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\nhi\n");
}

#[test]
fn alias_supports_the_symbol_spelling_of_both_names() {
    let result = run_ruby(
        r#"
        class Greeter
          def hello
            "hi"
          end
          alias :bonjour :hello
        end
        puts Greeter.new.bonjour
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\n");
}

#[test]
fn alias_preserves_the_original_methods_own_parameters() {
    let result = run_ruby(
        r#"
        class Calc
          def add(a, b)
            a + b
          end
          alias sum add
        end
        puts Calc.new.sum(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn alias_captures_the_overriding_bodys_own_behavior_in_a_subclass() {
    // `alias` binds to whichever body is ALREADY in effect at the alias
    // statement's own position -- here, `Dog`'s own override (found in
    // `Dog`'s `own_methods`), not `Animal`'s.
    let result = run_ruby(
        r#"
        class Animal
          def speak
            "generic"
          end
        end
        class Dog < Animal
          def speak
            "woof"
          end
          alias original_speak speak
        end
        d = Dog.new
        puts d.speak
        puts d.original_speak
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "woof\nwoof\n");
}

#[test]
fn class_shift_self_defines_multiple_class_methods_at_once() {
    let result = run_ruby(
        r#"
        class MathUtils
          class << self
            def square(x)
              x * x
            end
            def cube(x)
              x * x * x
            end
          end
        end
        puts MathUtils.square(4)
        puts MathUtils.cube(3)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "16\n27\n");
}

#[test]
fn multi_assign_into_attr_targets_correctly_calls_the_setter() {
    // A real, previously-undetected bug: `CallTargetNode::name()` is
    // ALREADY the setter name (`:x=`, confirmed via `Prism.parse`), but
    // `parse::lower_multi_target` appended ANOTHER `=`, building an
    // unresolvable `x==` method name -- so ANY multi-assignment into two
    // attr targets (`b.x, b.y = b.y, b.x`, the idiomatic in-place swap)
    // panicked with a confusing "unsupported call `x==`" instead of
    // swapping the values. Array-index multi-assign targets (`arr[0],
    // arr[1] = ...`) were unaffected (a distinct code path, `[]=`, not
    // string-formatted from a name at all).
    let result = run_ruby(
        r#"
        arr = [1, 2, 3, 4]
        arr[0], arr[1] = arr[1], arr[0]
        puts arr[0]
        puts arr[1]

        class Box
          attr_accessor :x, :y
          def initialize(x, y)
            @x = x
            @y = y
          end
        end
        b = Box.new(1, 2)
        b.x, b.y = b.y, b.x
        puts b.x
        puts b.y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n1\n2\n1\n");
}

#[test]
fn user_defined_operator_methods_can_be_defined_and_dispatched() {
    // A real, previously-undetected bug: `safe_ident` had no escaping at
    // all for a method name that's a bare operator symbol (`+`, `<=>`,
    // `[]`, ...) -- `def +(other)` panicked at CODEGEN time with a raw
    // `proc_macro2` "not a valid Ident" error. This meant user-defined
    // operator overloading -- claimed since Phase 1 to "work for free"
    // once operators became ordinary Calls -- never actually worked for
    // the DEFINING side; every existing operator test exercised only
    // native `Int`/`Float` fast paths, which bypass `safe_ident` entirely.
    // Fixed via a fixed lookup table (`OPERATOR_METHOD_NAMES`) mapping
    // every operator symbol to a valid Rust identifier, consulted by BOTH
    // the method-definition side and the general-dispatch call site (the
    // same function backs both), so they agree automatically.
    let result = run_ruby(
        r#"
        class Vector
          attr_reader :x, :y
          def initialize(x, y)
            @x = x
            @y = y
          end
          def +(other)
            Vector.new(@x + other.x, @y + other.y)
          end
          def <=>(other)
            (@x * @x + @y * @y) <=> (other.x * other.x + other.y * other.y)
          end
          def to_s
            "(#{@x}, #{@y})"
          end
        end
        v1 = Vector.new(1, 2)
        v2 = Vector.new(3, 4)
        v3 = v1 + v2
        puts v3.to_s
        puts(v1 <=> v2)
        puts(v2 <=> v1)
        puts(v1 <=> v1)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "(4, 6)\n-1\n1\n0\n");
}

#[test]
fn attr_reader_and_attr_writer_declared_standalone_generate_only_their_own_half() {
    let result = run_ruby(
        r#"
        class Point
          attr_reader :x
          attr_writer :y
          def initialize(x, y)
            @x = x
            @y = y
          end
          def show_y
            @y
          end
        end
        p1 = Point.new(1, 2)
        puts p1.x
        p1.y = 99
        puts p1.show_y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n99\n");
}

#[test]
fn combined_optional_rest_post_keyword_keyword_rest_and_block_params_all_bind_correctly() {
    let result = run_ruby(
        r#"
        class Widget
          def build(a, b = 10, *rest, c, d: 5, **kw, &blk)
            puts a
            puts b
            puts rest
            puts c
            puts d
            puts kw[:e]
            puts blk.call
          end
        end
        Widget.new.build(1, 2, 3, 4, 5, d: 99, e: 100) { "block!" }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n3\n4\n5\n99\n100\nblock!\n");
}

#[test]
fn a_user_defined_freeze_override_wins_over_the_builtin() {
    // `freeze`/`frozen?` are ordinary overridable Kernel methods in real
    // Ruby -- a class's own definition must win over the universal dispatch.
    let result = run_ruby(
        r#"
        class Custom
          def freeze
            :custom
          end
        end
        puts Custom.new.freeze
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "custom\n");
}

#[test]
fn zero_arg_resume_binds_nil_block_params() {
    let result = run_ruby(
        r#"
        f = Fiber.new { |x| x.nil? }
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn yaml_load_and_dump_match_ruby_via_the_psych_alias() {
    // `require "yaml"` exposes both `YAML` and `Psych` (Ruby's `yaml.rb` is
    // `YAML = Psych`); dump uses Psych's block style (sequences under a key
    // stay at the key's indent).
    let result = run_ruby(
        r#"
        require "yaml"
        print YAML.dump({"a" => 1, "b" => [2, "x"]})
        puts YAML.load("list:\n- a\n- b").inspect
        puts Psych.load("x: 1").inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "---\na: 1\nb:\n- 2\n- x\n{\"list\" => [\"a\", \"b\"]}\n{\"x\" => 1}\n"
    );
}

#[test]
fn a_param_referenced_only_inside_an_escaping_block_is_captured() {
    // The Phase 14.4 capture fix: `n` (a method param) appears ONLY inside
    // the escaping block -- previously misclassified as a block-own local
    // and silently re-declared Nil. Oracle: 11, 12.
    let result = run_ruby(
        r#"
            class Pair
              def each(&blk)
                blk.call(1)
                blk.call(2)
                self
              end
              def add_all(n)
                each { |x| puts x + n }
              end
              def mapped(&blk)
                result = []
                each { |x| result << blk.call(x) }
                result
              end
            end
            Pair.new.add_all(10)
            puts Pair.new.mapped { |x| x * 3 }.length
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "11\n12\n2\n");
}

#[test]
fn new_binds_optional_initialize_params() {
    // The emit_new fix: one `initialize(items = nil)` serving both
    // `Bag.new` and `Bag.new([1])` (oracle: 0, 1).
    let result = run_ruby(
        r#"
            class Bag
              def initialize(items = nil)
                @n = items.nil? ? 0 : 1
              end
              def n
                @n
              end
            end
            puts Bag.new.n
            puts Bag.new([1]).n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n1\n");
}

#[test]
fn kwargs_and_double_splats_preserve_source_order() {
    // The KwArg merge: `Call`/`HashLit`/`Yield` carry ONE ordered list of
    // literal pairs and `**h` double-splats. Ruby's Hash is insertion-ordered
    // and the merge is left-to-right last-wins, so the interleaving is
    // observable. The old two-field split got the order wrong for a splat
    // before a pair and couldn't represent two splats at all. All
    // oracle-verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        def capture(**h) = h
        p capture(**{a: 1, b: 2}, c: 3)   # splat before pair
        p capture(c: 3, **{a: 1, b: 2})   # pair before splat
        p capture(**{x: 1}, **{y: 2})     # two splats
        p capture(**{x: 1}, **{x: 2})     # last-wins on dup key
        p capture(**{a: 1}, b: 2, **{c: 3})
        h = {a: 1}
        p({**h, b: 2})
        p({b: 2, **h})
        def y
          yield 1, **{k: 2, j: 3}
        end
        y { |*a| p a }
        class HasToHash
          def to_hash = {z: 99}
        end
        p capture(**HasToHash.new, w: 1)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{a: 1, b: 2, c: 3}\n\
         {c: 3, a: 1, b: 2}\n\
         {x: 1, y: 2}\n\
         {x: 2}\n\
         {a: 1, b: 2, c: 3}\n\
         {a: 1, b: 2}\n\
         {b: 2, a: 1}\n\
         [1, {k: 2, j: 3}]\n\
         {z: 99, w: 1}\n"
    );
}

#[test]
fn initialize_takes_the_full_param_shapes() {
    // `.new` was the last call site still binding arguments by hand, and it
    // only handled required + optional -- `def initialize(*values)` was a
    // flat compile-time rejection.
    let result = run_ruby(
        r#"
        class Splat
          def initialize(*v); @v = v; end
          attr_reader :v
        end
        p Splat.new.v
        p Splat.new(1).v
        p Splat.new(1, 2, 3).v

        class Mixed
          def initialize(a, b = 5, *rest, last)
            @all = [a, b, rest, last]
          end
          attr_reader :all
        end
        p Mixed.new(1, 9).all
        p Mixed.new(1, 2, 3, 4, 9).all

        class Kw
          def initialize(a, k:, j: 7, **rest)
            @all = [a, k, j, rest]
          end
          attr_reader :all
        end
        p Kw.new(1, k: 2).all
        p Kw.new(1, k: 2, j: 3, z: 4).all

        class Post
          def initialize(a, *m, y, z); @all = [a, m, y, z]; end
          attr_reader :all
        end
        p Post.new(1, 2, 3, 4, 5).all
        p Post.new(1, 2, 3).all
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[]\n[1]\n[1, 2, 3]\n[1, 5, [], 9]\n[1, 2, [3, 4], 9]\n\
         [1, 2, 7, {}]\n[1, 2, 3, {z: 4}]\n[1, [2, 3], 4, 5]\n[1, [], 2, 3]\n"
    );
}

#[test]
fn redefining_a_method_in_one_class_body_last_def_wins() {
    let result = run_ruby(
        r#"
        class Foo
          def a
            "first"
          end

          def a
            "second"
          end
        end

        puts Foo.new.a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "second\n");
}

#[test]
fn bare_super_forwards_current_arguments() {
    // Forwards the CURRENT bindings (the reassigned `name`), through
    // required and optional params alike.
    let result = run_ruby(
        r#"
        class A
          def greet(name, punct = ".")
            "hi #{name}#{punct}"
          end
        end

        class B < A
          def greet(name, punct = ".")
            name = name + "!"
            super
          end
        end

        puts B.new.greet("bob")
        puts B.new.greet("bob", "?")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi bob!.\nhi bob!?\n");
}

#[test]
fn bare_super_forwards_rest_and_keyword_params() {
    let result = run_ruby(
        r#"
        class C
          def count(*nums)
            nums.length
          end
        end

        class D < C
          def count(*nums)
            super * 10
          end
        end

        puts D.new.count(1, 2, 3)

        class E
          def kw(a:, b: 2)
            "a=#{a} b=#{b}"
          end
        end

        class F < E
          def kw(a:, b: 2)
            "got " + super
          end
        end

        puts F.new.kw(a: 1, b: 9)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "30\ngot a=1 b=9\n");
}

#[test]
fn super_with_empty_parens_passes_no_arguments() {
    // `super()` and bare `super` mean OPPOSITE things: the parens form
    // passes nothing, so the parent's optional takes its default.
    let result = run_ruby(
        r#"
        class G
          def greet(name = "anon")
            "hi #{name}"
          end
        end

        class H < G
          def greet(name)
            super() + "!"
          end
        end

        puts H.new.greet("bob")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi anon!\n");
}

#[test]
fn a_user_defined_dup_override_wins() {
    // `dup`/`clone` are ordinary overridable Kernel methods in real Ruby --
    // the static arm must fall through to Path 1 dispatch when the
    // receiver's class defines its own.
    let result = run_ruby(
        r#"
        class W
          def dup
            "custom"
          end
        end

        puts W.new.dup
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "custom\n");
}

#[test]
fn qualified_definition_form_does_not_see_the_namespace_lexically() {
    // `class Store::Cart`'s cref is just [Cart] -- real Ruby raises
    // NameError for `DEFAULT`, naming the cref head's qualified path.
    let result = run_ruby(
        r#"
        module Store
          DEFAULT = 10
        end

        class Store::Cart
          def d
            DEFAULT
          rescue NameError => e
            "NameError: #{e.message}"
          end
        end

        puts Store::Cart.new.d
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "NameError: uninitialized constant Store::Cart::DEFAULT\n"
    );
}

#[test]
fn nested_classes_reopen_through_both_definition_forms() {
    let result = run_ruby(
        r#"
        module Store
          class Item
            def tag
              "tagged"
            end
          end
        end

        module Store
          class Item
            def more
              "reopened nested"
            end
          end
        end

        class Store::Item
          def qual
            "qualified reopen"
          end
        end

        i = Store::Item.new
        puts i.tag
        puts i.more
        puts i.qual
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "tagged\nreopened nested\nqualified reopen\n");
}

// -- Phase 16.2: user-overridable object protocols + Comparable. Every
// expectation oracle-verified against real ruby 4.0.5.

#[test]
fn user_defined_equality_dispatches_everywhere() {
    let result = run_ruby(
        r#"
        class Point
          attr_reader :x

          def initialize(x)
            @x = x
          end

          def ==(other)
            other.is_a?(Point) && x == other.x
          end
        end

        puts Point.new(1) == Point.new(1)
        puts Point.new(1) == Point.new(2)
        puts Point.new(1) == 5
        puts Point.new(1) != Point.new(2)
        puts [Point.new(1), Point.new(2)] == [Point.new(1), Point.new(2)]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\nfalse\ntrue\ntrue\n");
}

#[test]
fn collections_compare_element_wise_and_objects_default_to_identity() {
    let result = run_ruby(
        r#"
        puts [1, [2, 3]] == [1, [2, 3]]
        puts({ a: 1, b: [2] } == { a: 1, b: [2] })
        puts({ a: 1 } == { a: 2 })

        class Blank
        end

        b1 = Blank.new
        b2 = Blank.new
        puts b1 == b1
        puts b1 == b2
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\ntrue\nfalse\n");
}

/// Full `Params` support on a reopen method (required + rest), plus
/// `respond_to?` seeing value methods through the widened registry probe.
#[test]
fn builtin_reopen_rest_params_and_respond_to() {
    let result = run_ruby(
        r#"
        class String
          def tag(first, *rest)
            label = first.to_s
            rest.each do |r|
              label = label + "|" + r.to_s
            end
            label + ":" + self
          end
        end

        puts "v".tag("a", "b", "c")
        puts "w".tag("z")
        puts "x".respond_to?(:tag)
        puts "x".respond_to?(:zzz)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a|b|c:v\nz:w\ntrue\nfalse\n");
}

/// The send-ladder fidelity fix: a REAL module method (Enumerable's `map`,
/// via `include Enumerable` + `each`) resolves BEFORE `method_missing` --
/// previously method_missing fired first, the opposite of real Ruby.
#[test]
fn enumerable_resolves_before_method_missing() {
    let result = run_ruby(
        r#"
        class Sack
          include Enumerable
          def initialize(items)
            @items = items
          end
          def each(&b)
            @items.each(&b)
            self
          end
          def method_missing(name, *a)
            "mm:#{name}"
          end
        end

        s = Sack.new([1, 2, 3])
        puts s.map { |x| x * 10 }.inspect
        puts s.nope
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[10, 20, 30]\nmm:nope\n");
}

/// The interception-order fix: a user-defined sibling `puts` now WINS over
/// the Kernel function (real Ruby's rule; the old intercept-first order
/// was a latent bug).
#[test]
fn a_user_defined_puts_wins_over_the_kernel_function() {
    let result = run_ruby(
        r#"
        class Logger
          def puts(msg)
            $stdout_lines = 1
            "logged: #{msg}"
          end
          def run
            puts("hi")
          end
        end

        v = Logger.new.run
        p v
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"logged: hi\"\n");
}

/// `Data.define` outside a constant assignment mints a native immutable data
/// class -- keyword construction, `with`, frozen, byte-exact `inspect`.
#[test]
fn anonymous_data_define_mints_a_native_immutable_class() {
    let result = run_ruby(
        r#"
        d = Data.define(:x, :y)
        pt = d.new(x: 1, y: 2)
        p pt
        p pt.x
        p pt.to_h
        p pt.with(x: 9)
        p pt.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<data x=1, y=2>\n1\n{x: 1, y: 2}\n#<data x=9, y=2>\ntrue\n"
    );
}

/// #192 feature (1): a `super` inside a method defined in a NON-const
/// `Struct.new`/`Data.define` block resolves through the runtime method-frame
/// stack into the native member-binding `initialize`, where previously it
/// panicked "`super` outside a method" at codegen. Bare (zsuper) and explicit
/// positional forms, plus the keyword form for Data.
#[test]
fn anonymous_struct_and_data_custom_initialize_super() {
    let result = run_ruby(
        r#"
        pair = Struct.new(:x, :y) do
          def initialize(x)
            super(x, x * 2)
          end
        end
        p pair.new(5).to_a

        echo = Struct.new(:a, :b) do
          def initialize(a, b)
            super
          end
        end
        p echo.new(1, 2).to_a

        coord = Data.define(:lat, :lng) do
          def initialize(lat:, lng:)
            super(lat: lat * 10, lng: lng)
          end
        end
        p coord.new(lat: 1, lng: 2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[5, 10]\n[1, 2]\n#<data lat=10, lng=2>\n"
    );
}

/// #192 feature (1): a `super` in an override installed via a `Class.new(Parent)`
/// block reaches the parent's method, when the parent is a runtime class. The
/// override forwards args and composes the parent's result.
#[test]
fn class_new_override_supers_into_a_runtime_parent() {
    let result = run_ruby(
        r#"
        base = Class.new do
          def greet(n)
            "hi #{n}"
          end
        end
        sub = Class.new(base) do
          def greet(n)
            super(n) + "!"
          end
        end
        p sub.new.greet("x")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"hi x!\"\n");
}

// --- Top-level method definitions (G0) ---------------------------------
//
// A top-level `def` is a PRIVATE instance method on `Object` (real Ruby's
// rule): registered on arena slot 0, materialized by `analyze::mro` into
// every user class (so any method body reaches it via implicit self, with
// `@ivar`s landing on the calling class's own struct), and emitted through
// the builtin-reopen container (`__bm_Object`) whose copies dispatch on
// the runtime `main` object at top-level call sites.

#[test]
fn top_level_def_defines_and_calls() {
    let result = run_ruby(
        r#"
        def greet(name, punct = "!")
          "hi #{name}#{punct}"
        end
        puts greet("world")
        puts greet("you", "?")
        "#,
    );
    assert_eq!(result.stdout, "hi world!\nhi you?\n");
    assert_eq!(result.stderr, "");
}

#[test]
fn top_level_endless_def() {
    let result = run_ruby("def double(x) = x * 2\nputs double(21)\n");
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn top_level_def_recursion_and_mutual_calls() {
    let result = run_ruby(
        r#"
        def fib(n)
          n < 2 ? n : fib(n - 1) + fib(n - 2)
        end
        def announce(n)
          puts "fib(#{n}) = #{fib(n)}"
        end
        announce(10)
        "#,
    );
    assert_eq!(result.stdout, "fib(10) = 55\n");
}

#[test]
fn top_level_def_reachable_from_instance_and_class_methods() {
    let result = run_ruby(
        r#"
        def helper(x)
          x + 1
        end
        class Widget
          def go
            helper(4)
          end
          def self.direct
            helper(10)
          end
        end
        puts Widget.new.go
        puts Widget.direct
        "#,
    );
    assert_eq!(result.stdout, "5\n11\n");
    assert_eq!(result.stderr, "");
}

#[test]
fn top_level_def_with_block_and_yield() {
    let result = run_ruby(
        r#"
        def twice
          yield 1
          yield 2
        end
        twice { |i| puts "got #{i}" }
        "#,
    );
    assert_eq!(result.stdout, "got 1\ngot 2\n");
}

#[test]
fn top_level_def_called_from_a_lambda() {
    let result = run_ruby(
        r#"
        def base
          40
        end
        f = ->(x) { base + x }
        puts f.call(2)
        "#,
    );
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn top_level_def_overrides_a_kernel_function() {
    // Sibling/top-level resolution runs BEFORE the Kernel function set --
    // real Ruby's rule (a user `def rand` wins over Kernel#rand).
    let result = run_ruby(
        r#"
        def rand
          7
        end
        puts rand
        "#,
    );
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn top_level_def_last_def_wins() {
    let result = run_ruby(
        r#"
        def v
          1
        end
        def v
          2
        end
        puts v
        "#,
    );
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn top_level_def_self_defines_a_singleton_method_on_main() {
    // A top-level `def self.name` is a SINGLETON method on `main` (Batch G) --
    // callable via implicit self at the top level, but (CRuby's asymmetry with
    // a plain top-level `def`, a private Object instance method) NOT from
    // inside another object's method, where self isn't `main`.
    let result = run_ruby(
        r#"
        def self.only_main; "main-only"; end
        def self.wrap; "[#{yield}]"; end
        puts only_main
        puts wrap { "b" }
        class Widget
          def try; only_main rescue "not-visible"; end
        end
        puts Widget.new.try
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "main-only\n[b]\nnot-visible\n");
}

#[test]
fn array_new_takes_a_size_default_and_block() {
    // The block form has to reach the runtime allocator, so `Array.new`
    // joins the block-keeping set in lowering -- routing it through
    // `HirNode::New` (which has no block slot) silently answered nils.
    let result = run_ruby(
        r#"
        p Array.new(3) { |i| i * 2 }
        p Array.new(2, "x")
        p Array.new(3)
        p Array.new
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[0, 2, 4]\n[\"x\", \"x\"]\n[nil, nil, nil]\n[]\n"
    );
}

#[test]
fn block_params_are_reassignable() {
    let result = run_ruby(
        r#"
        1.upto(2) { |n| n = n + 10; puts n }
        3.times { |i| i = i * 2; print i }
        puts
        "#,
    );
    assert_eq!(result.stdout, "11\n12\n024\n");
}

// --- This session's correctness fixes. Each covers a behavior the
// `examples/*.rb` fixtures also exercise end to end against ruby 4.0.5;
// these pin the specific shape that was broken, so a regression names
// itself rather than showing up as an example-wide diff.

#[test]
fn a_block_auto_splats_a_lone_array_across_its_positional_params() {
    // CRuby's `has_lead && !ambiguous_param0` rule -- see
    // `codegen::params::auto_splats`, whose unit tests cover every shape.
    let result = run_ruby(
        r#"
        def one(v)
          yield v
        end
        p(one([1, 2]) { |a, b| [a, b] })
        p(one([1, 2]) { |a| a })
        p(one([1, 2]) { |*a| a })
        p(one([1, 2]) { |a, *b| [a, b] })
        p(one([1, 2]) { |a, **k| [a, k] })
        p [[1, 2], [3, 4]].map { |a, b| a + b }
        p({ x: 1 }.map { |k, v| [k, v] })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[1, 2]\n[[1, 2]]\n[1, [2]]\n[[1, 2], {}]\n[3, 7]\n[[:x, 1]]\n"
    );
}

#[test]
fn a_lambda_never_auto_splats_and_checks_its_arity() {
    let result = run_ruby(
        r#"
        strict = ->(a, b) { [a, b] }
        p strict.call(1, 2)
        begin
          strict.call([1, 2])
        rescue ArgumentError => e
          puts "ArgumentError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2]\nArgumentError\n");
}

#[test]
fn proc_arity_and_lambda_p_report_the_blocks_own_signature() {
    let result = run_ruby(
        r#"
        p ->(a, b) {}.arity
        p ->(a, b = 1) {}.arity
        p proc { |a, b = 1| }.arity
        p proc { |a, *b| }.arity
        p ->(a, b:) {}.arity
        p ->(a, **k) {}.arity
        p ->() {}.lambda?
        p proc {}.lambda?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n-2\n1\n-2\n2\n-2\ntrue\nfalse\n");
}

#[test]
fn super_with_a_literal_block_drives_the_parents_yield() {
    let result = run_ruby(
        r##"
        class Parent
          def run
            yield
          end
          def each_twice
            yield 1
            yield 2
          end
        end
        class Child < Parent
          def run
            super { "from-child" }
          end
          def each_twice
            super { |v| puts "got #{v}" }
          end
        end
        puts Child.new.run
        Child.new.each_twice
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from-child\ngot 1\ngot 2\n");
}

#[test]
fn super_with_no_literal_block_forwards_the_callers_block() {
    let result = run_ruby(
        r#"
        class Parent
          def run
            yield
          end
        end
        class Child < Parent
          def run
            super
          end
        end
        puts(Child.new.run { "from-caller" })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from-caller\n");
}

#[test]
fn a_top_level_def_is_a_private_method_of_object() {
    // Callable by implicit self everywhere, invisible to `respond_to?`,
    // reachable via `send`, and a NoMethodError with an explicit receiver.
    let result = run_ruby(
        r##"
        def greet(name)
          "Hello, #{name}!"
        end
        class Speaker
          def speak
            greet("instance")
          end
        end
        puts greet("top")
        puts Speaker.new.speak
        p self.respond_to?(:greet)
        p self.respond_to?(:greet, true)
        p Speaker.new.respond_to?(:greet)
        p send(:greet, "send")
        p Speaker.new.send(:greet, "on-instance")
        begin
          Speaker.new.greet("explicit")
        rescue NoMethodError
          puts "NoMethodError"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Hello, top!\nHello, instance!\nfalse\ntrue\nfalse\n\"Hello, send!\"\n\"Hello, on-instance!\"\nNoMethodError\n"
    );
}

#[test]
fn a_top_level_def_is_callable_from_a_class_body_and_a_class_method() {
    // `self` in a class body/class method is the CLASS OBJECT -- there's no
    // concrete receiver to clone, so implicit-self dispatch must go through
    // `RubyValue::Class`, not an invalid `self.clone()`.
    let result = run_ruby(
        r##"
        def helper(tag)
          "helped-#{tag}"
        end
        class AtBody
          RESULT = helper("body")
        end
        class Factory
          def self.build
            helper("class-method")
          end
        end
        puts AtBody::RESULT
        puts Factory.build
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "helped-body\nhelped-class-method\n");
}

/// A parenthesized parameter destructures the value bound to its slot --
/// `|(a, b)|`, mixed with plain params, and nested arbitrarily deep. Lowered
/// as the multi-assignment it is (see `hir::Params::destructures`).
#[test]
fn parenthesized_block_params_destructure_their_slot() {
    let result = run_ruby(
        r#"
        [[1, 2], [3, 4]].each { |(a, b)| p [a, b] }
        [[1, [2, 3], 4]].each { |a, (b, c), d| p [a, b, c, d] }
        [[[1, 2], 3]].each { |(a, b), c| p [a, b, c] }
        [[1, [2, [3, 4]]]].each { |a, (b, (c, d))| p [a, b, c, d] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[3, 4]\n[1, 2, 3, 4]\n[1, 2, 3]\n[1, 2, 3, 4]\n"
    );
}

/// A destructuring param may carry a splat, named or anonymous -- the same
/// `before`/`splat`/`after` shape every multi-assignment target group has.
#[test]
fn destructuring_params_take_splats() {
    let result = run_ruby(
        r#"
        [[1, [2, 3, 4]]].each { |a, (b, *r)| p [a, b, r] }
        [[1, 2, 3]].each { |a, (*), b| p [a, b] }
        [[[1, 2, 3], 9]].each { |(*init, last), z| p [init, last, z] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2, [3, 4]]\n[1, 3]\n[[1, 2], 3, 9]\n");
}

/// METHOD params destructure too, not just block params.
#[test]
fn method_params_destructure() {
    // `r##"..."##`: the body contains `"#{a}`, which would close an `r#"..."#`.
    let result = run_ruby(
        r##"
        def pair((a, b)) = "#{a}-#{b}"
        puts pair([1, 2])
        def nested((a, (b, c))) = [a, b, c]
        p nested([1, [2, 3]])
        def mixed(x, (y, z)) = [x, y, z]
        p mixed(1, [2, 3])
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1-2\n[1, 2, 3]\n[1, 2, 3]\n");
}

/// A lone Array argument spreads across a block's positional params -- but a
/// block with exactly ONE plain param receives it whole (`ambiguous_param0`,
/// the `each { |pair| }` idiom), and so does a lone optional. Oracle-derived;
/// see `codegen::params::auto_splats` for the full truth table.
#[test]
fn block_auto_splat_follows_the_ambiguous_param0_rule() {
    let result = run_ruby(
        r#"
        def one(x) = yield x
        one([1, 2]) { |a| p a }
        one([1, 2]) { |a = 9| p a }
        one([1, 2]) { |*a| p a }
        one([1, 2]) { |a, **k| p a }
        one([1, 2]) { |a, b| p [a, b] }
        one([1, 2]) { |a, *b| p [a, b] }
        one([1, 2]) { |*a, b| p [a, b] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[1, 2]\n[[1, 2]]\n[1, 2]\n[1, 2]\n[1, [2]]\n[[1], 2]\n"
    );
}

/// A trailing comma (`|a, |`) means "this block takes more than one param",
/// which turns auto-splat on and then discards everything past the named
/// ones -- exactly an anonymous rest, which is how it lowers.
#[test]
fn a_trailing_comma_param_is_an_anonymous_rest() {
    let result = run_ruby(
        r#"
        def one(x) = yield x
        one([1, 2]) { |a,| p a }
        one([1, 2, 3]) { |a, b,| p [a, b] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n[1, 2]\n");
}

#[test]
fn define_singleton_method_forms() {
    // In-body (bare and self), external constant receiver (with a param),
    // and a namespaced receiver all define a class method.
    let result = run_ruby(
        r#"
        class C
          define_singleton_method(:a) { "a" }
          self.define_singleton_method(:b) { "b" }
        end
        C.define_singleton_method(:c) { |n| n + 1 }
        module M
          class D; end
        end
        M::D.define_singleton_method(:d) { "nested" }
        puts C.a
        puts C.b
        puts C.c(41)
        puts M::D.d
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n42\nnested\n");
}

#[test]
fn alias_and_alias_method_including_inherited_sources() {
    // Same-class and inherited sources, both spellings, chained across levels.
    let result = run_ruby(
        r#"
        class Base
          def greet(who); "hi " + who; end
          alias hail greet
          alias_method :salute, :greet
        end
        class Mid < Base
          alias_method :welcome, :greet
          alias hey greet
        end
        class Leaf < Mid
          alias again welcome
        end
        puts Base.new.hail("a")
        puts Base.new.salute("b")
        puts Mid.new.welcome("c")
        puts Mid.new.hey("d")
        puts Leaf.new.again("e")
        p Leaf.method_defined?(:again)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hi a\nhi b\nhi c\nhi d\nhi e\ntrue\n"
    );
}

#[test]
fn hash_new_default_value_and_default_block() {
    let result = run_ruby(
        r#"
        h = Hash.new(0)
        h[:a] += 1
        h[:a] += 1
        p h[:a]
        p h[:z]
        p h.size
        d = Hash.new { |hh, k| hh[k] = k.to_s }
        p d[:q]
        p d
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n0\n1\n\"q\"\n{q: \"q\"}\n");
}

#[test]
fn eval_locals_defined_and_read_within_one_scope() {
    let result = run_ruby(r#"src = "a = 4; b = 5; a * b"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n");
}

#[test]
fn module_method_defined_accepts_the_inherit_flag() {
    let result = run_ruby(
        r#"
        class Foo; def bar; end; end
        p Foo.method_defined?(:bar)
        p Foo.method_defined?(:bar, true)
        p Foo.method_defined?(:nope, false)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\n");
}

#[test]
fn method_arity_reflects_the_keyword_shape() {
    // A required keyword adds one fixed mandatory slot; an optional keyword or
    // keyword-rest with NO required keyword makes the method variadic. An
    // optional positional or rest is variadic; a block never affects arity.
    let result = run_ruby(
        r#"
        def none; end
        def two(a, b); end
        def opt(x, y = 1); end
        def splat(*xs); end
        def post_splat(a, *b, c); end
        def opt_kw(a, b: 1); end
        def req_kw(a, b:); end
        def mix_kw(a, b:, c: 1); end
        def kw_rest(a, **kw); end
        def req_kw_rest(a, b:, **kw); end
        def with_blk(a, &blk); end
        p [method(:none).arity, method(:two).arity, method(:opt).arity,
           method(:splat).arity, method(:post_splat).arity, method(:opt_kw).arity,
           method(:req_kw).arity, method(:mix_kw).arity, method(:kw_rest).arity,
           method(:req_kw_rest).arity, method(:with_blk).arity]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[0, 2, -2, -1, -3, -2, 2, 2, -2, 2, 1]\n");
}

#[test]
fn method_arity_for_builtins_and_accessors() {
    // Builtin-receiver method objects read a dumped CRuby arity table; attr and
    // struct accessors derive theirs from the accessor shape (reader 0, writer 1).
    let result = run_ruby(
        r#"
        p "hello".method(:upcase).arity
        p 5.method(:+).arity
        p [1].method(:size).arity
        class C; attr_accessor :x; end
        p C.new.method(:x).arity
        p C.new.method(:x=).arity
        S = Struct.new(:a)
        s = S.new(1)
        p s.method(:a).arity
        p s.method(:a=).arity
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "-1\n1\n0\n0\n1\n0\n1\n");
}

#[test]
fn module_method_defined_inherit_false_restricts_to_own_methods() {
    // `inherit: false` reports only methods the receiver defines directly --
    // its own defs and attr accessors -- skipping the ancestor walk.
    let result = run_ruby(
        r#"
        class Animal; def name; end; attr_accessor :age; end
        class Dog < Animal; def bark; end; end
        p Dog.method_defined?(:bark, false)
        p Dog.method_defined?(:name, false)
        p Dog.method_defined?(:age=, false)
        p Animal.method_defined?(:age, false)
        p Animal.method_defined?(:bark, false)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\nfalse\ntrue\nfalse\n");
}

#[test]
fn proc_parameters_lambda_keyword_forces_the_reporting_view() {
    // `parameters(lambda:)` forces the view: true reports plain positionals as
    // :req, false as :opt, nil follows the receiver. A defaulted positional
    // stays :opt in every view; a bound method's to_proc keeps the method arity.
    let result = run_ruby(
        r#"
        p proc { |x, y| }.parameters(lambda: true)
        p ->(x, y) { }.parameters(lambda: false)
        p proc { |x, y = 1| }.parameters(lambda: true)
        p proc { |x| }.parameters(lambda: nil)
        p lambda { _1 }.parameters
        def greet(name); end
        gp = method(:greet).to_proc
        p [gp.arity, gp.lambda?]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[[:req, :x], [:req, :y]]\n\
         [[:opt, :x], [:opt, :y]]\n\
         [[:req, :x], [:opt, :y]]\n\
         [[:opt, :x]]\n\
         [[:req, :_1]]\n\
         [1, true]\n",
    );
}

#[test]
fn proc_parameters_reflect_the_signature() {
    let result = run_ruby(
        r#"
        p proc { |x, y| }.parameters
        p lambda { |x, y| }.parameters
        p proc { |a, b = 1, *c, d, k:, m: 2, **n, &blk| }.parameters
        p ->(a, b) { }.parameters
        p proc { |*| }.parameters
        p proc { |**| }.parameters
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[[:opt, :x], [:opt, :y]]\n\
         [[:req, :x], [:req, :y]]\n\
         [[:opt, :a], [:opt, :b], [:rest, :c], [:opt, :d], [:keyreq, :k], [:key, :m], [:keyrest, :n], [:block, :blk]]\n\
         [[:req, :a], [:req, :b]]\n\
         [[:rest, :*]]\n\
         [[:keyrest, :**]]\n",
    );
}

#[test]
fn three_way_method_visibility_reflection() {
    // Private/public/protected are tracked distinctly: `instance_methods`
    // keeps public+protected, the prefixed queries match exactly one
    // visibility, `respond_to?`'s default skips private AND protected, and a
    // subclass may re-declare an inherited method's visibility.
    let result = run_ruby(
        r#"
        class Account
          def deposit; end
          private
          def log; end
          protected
          def compare; end
        end
        def s(a); a.map(&:to_s).sort; end
        p s(Account.instance_methods(false))
        p s(Account.public_instance_methods(false))
        p s(Account.protected_instance_methods(false))
        p s(Account.private_instance_methods(false))
        p Account.public_method_defined?(:deposit)
        p Account.protected_method_defined?(:compare)
        p Account.private_method_defined?(:log)
        p Account.public_method_defined?(:compare)
        a = Account.new
        p a.respond_to?(:compare)
        p a.respond_to?(:compare, true)
        class Base
          def m; end
        end
        class Sub < Base
          private :m
        end
        p Sub.new.respond_to?(:m)
        p Sub.private_method_defined?(:m)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"compare\", \"deposit\"]\n[\"deposit\"]\n[\"compare\"]\n[\"log\"]\n\
         true\ntrue\ntrue\nfalse\nfalse\ntrue\nfalse\ntrue\n",
    );
}

#[test]
fn super_resolves_into_an_included_module() {
    // A compile-time scan of the SUPERCLASS chain alone misses a method that
    // only an included module defines; the runtime walk covers the real
    // ancestry, so both a class->module and a module->module `super` land.
    let result = run_ruby(
        r#"
        module Greet
          def hi
            "[hi]"
          end
        end
        class C
          include Greet
          def hi
            super
          end
        end
        module M1
          def tag
            "M1"
          end
        end
        module M2
          include M1
          def tag
            "M2(#{super})"
          end
        end
        class F
          include M2
          def tag
            "F[#{super}]"
          end
        end
        puts C.new.hi
        puts F.new.tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[hi]\nF[M2(M1)]\n");
}

#[test]
fn method_missing_receiver_descriptions_match_cruby() {
    // One raiser builds every NoMethodError message, so the four receiver
    // shapes stay in step: an instance, nil (rendered bare, no "an instance
    // of" prefix), a module, and a class. Critically the receiver is described
    // by CLASS NAME and never by calling `inspect` on it -- CRuby's formats
    // carry no receiver-inspect directive, which is what lets an object with
    // no `inspect` still raise an error about itself.
    let result = run_ruby(
        r#"
        def msg
          yield
        rescue NoMethodError => e
          puts e.message
        end
        module Helper; end
        msg { Object.new.nope }
        msg { nil.nope }
        msg { Helper.nope }
        msg { String.nope }
        msg { 5.nope }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "undefined method 'nope' for an instance of Object\n\
         undefined method 'nope' for nil\n\
         undefined method 'nope' for module Helper\n\
         undefined method 'nope' for class String\n\
         undefined method 'nope' for an instance of Integer\n",
    );
}

#[test]
fn public_send_enforces_visibility_at_runtime() {
    // `public_send` is stricter than an ordinary explicit-receiver call: it
    // rejects private AND protected, with no self-relatedness relaxation,
    // because CRuby implements it by passing `Qundef` as the caller's self
    // (vm_eval.c:1230) -- a sentinel nothing can be a kind of. This is a
    // RUNTIME check (rb_method_call_status), so the program must compile and
    // the NoMethodError must be rescuable. Covers explicit-receiver, implicit
    // self, and a runtime-computed method name.
    let result = run_ruby(
        r##"
        def err
          yield
        rescue NoMethodError => e
          "#{e.class}: #{e.message}"
        end
        class Acct
          def balance = 100
          def peer_check(other) = other.guarded
          protected
          def guarded = "prot"
          private
          def secret = 42
        end
        class Inner
          def run = public_send(:hidden)
          private
          def hidden = 1
        end
        def dyn(o, m) = o.public_send(m)
        a = Acct.new
        p a.public_send(:balance)
        puts err { a.public_send(:secret) }
        puts err { a.public_send(:guarded) }
        p a.send(:secret)
        p a.peer_check(Acct.new)
        puts err { Inner.new.run }
        puts err { dyn(a, :secret) }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "100\n\
         NoMethodError: private method 'secret' called for an instance of Acct\n\
         NoMethodError: protected method 'guarded' called for an instance of Acct\n\
         42\n\
         \"prot\"\n\
         NoMethodError: private method 'hidden' called for an instance of Inner\n\
         NoMethodError: private method 'secret' called for an instance of Acct\n",
    );
}

#[test]
fn define_method_with_a_symbol_to_proc_block() {
    // `define_method(:name, &:other)`. In CRuby the `&` conversion happens at
    // the CALL SITE, before rb_mod_define_method runs (proc.c:2872, which
    // rejects a bare Symbol as its second positional arg), so the body is the
    // symbol proc `->(recv, *rest) { recv.other(*rest) }` -- which is why the
    // defined method takes its RECEIVER as the first argument.
    let result = run_ruby(
        r#"
        class Widget
          define_method(:as_str, &:to_s)
        end
        class Gadget
          define_method :label, &:to_s
        end
        puts Widget.new.as_str(7)
        puts Gadget.new.label(8)
        p [1, 2, 3].map(&:to_s)
        p [10, 20, 30].inject(0, &:+)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n8\n[\"1\", \"2\", \"3\"]\n60\n");
}

#[test]
fn super_resolves_in_a_class_method() {
    // In Ruby `def self.foo` is an ordinary instance method on the SINGLETON
    // class, and singleton classes form a parallel chain --
    // #<Class:C>.super == #<Class:C.superclass> (make_metaclass,
    // class.c:1186) -- so `super` needs no special case. Covers a three-level
    // chain, explicit args, and zsuper forwarding of a defaulted parameter.
    let result = run_ruby(
        r#"
        class Foo
          def self.base; 100; end
          def self.greet(name); "hello #{name}"; end
          def self.tag(pfx = "t"); pfx + "-foo"; end
        end
        class Bar < Foo
          def self.base; super * 10; end
          def self.greet(name); super(name.upcase) + "!"; end
          def self.tag(pfx = "t"); super; end
        end
        class Baz < Bar
          def self.base; super + 1; end
        end
        p Foo.base
        p Bar.base
        p Baz.base
        p Bar.greet("matz")
        p Bar.tag
        p Bar.tag("x")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "100\n1000\n1001\n\"hello MATZ!\"\n\"t-foo\"\n\"x-foo\"\n",
    );
}

/// `super` reaching the root `initialize` -- what every chain bottoms out on,
/// and what makes the ordinary `include SomeMixin` + `super` idiom work. It
/// used to raise "no superclass method 'initialize'": the runtime `super` walk
/// consulted only the registry, never the builtin table. Arity 0 is enforced,
/// and `initialize` stays private to reflection.
#[test]
fn super_reaches_the_root_initialize() {
    let result = run_ruby(
        r##"
        module Greet
          def initialize
            super
            @greeted = true
          end
          def greeted?; @greeted; end
        end
        class Person
          include Greet
        end
        p Person.new.greeted?

        class Strict
          def initialize(x); super; end
        end
        begin
          Strict.new(1)
        rescue ArgumentError => e
          p e.message
        end

        class Ok
          def initialize(x); super(); @x = x; end
          attr_reader :x
        end
        p Ok.new(42).x

        p Object.new.respond_to?(:initialize)
        p Object.new.respond_to?(:initialize, true)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\n\"wrong number of arguments (given 1, expected 0)\"\n42\nfalse\ntrue\n"
    );
}

#[test]
fn singleton_method_object_binds_a_named_block_param() {
    // A `&blk` param on a per-object singleton binds to the method's call-site
    // block (nil when none is passed), not the unconditional nil it used to.
    let result = run_ruby(
        r#"
        obj = Object.new
        def obj.wrap(&blk)
          blk.nil? ? "no block" : blk.call(41) + 1
        end
        puts obj.wrap { |n| n }
        puts obj.wrap
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\nno block\n");
}

#[test]
fn singleton_class_object_yields_inside_class_shift_obj() {
    let result = run_ruby(
        r#"
        obj = Object.new
        class << obj
          def each_pair
            yield 1
            yield 2
            block_given? ? "done" : "noblock"
          end
        end
        collected = []
        r = obj.each_pair { |n| collected << n }
        p collected
        puts r
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2]\ndone\n");
}

#[test]
fn def_in_class_new_block_yields() {
    // A `def` in expression position (a `Class.new` block) now threads its
    // block too -- previously a `panic!`/spike-scope rejection.
    let result = run_ruby(
        r#"
        k = Class.new do
          def run
            yield 10
          end
        end
        puts k.new.run { |x| x * 2 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n");
}

#[test]
fn singleton_method_yields_params_and_multiple_values() {
    let result = run_ruby(
        r##"
        obj = Object.new
        def obj.combine(a, b)
          yield a, b, a + b
        end
        obj.combine(2, 3) { |x, y, s| puts "#{x} #{y} #{s}" }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2 3 5\n");
}

#[test]
fn singleton_method_inner_block_yields_to_the_methods_own_block() {
    // Inside a singleton method, an ORDINARY block's `yield` targets the
    // method's own (call-site) block -- the lexical-clone path composing over
    // the method-body lambda's call-site `__blk`.
    let result = run_ruby(
        r##"
        obj = Object.new
        def obj.sum_pairs
          total = 0
          [1, 2, 3].each { |n| total += yield(n) }
          total
        end
        p obj.sum_pairs { |n| n * 10 }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "60\n");
}

#[test]
fn top_level_def_self_singletons_can_call_each_other_and_yield() {
    let result = run_ruby(
        r##"
        def self.outer
          "o:#{inner { 1 }}"
        end
        def self.inner
          yield + 41
        end
        puts outer
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "o:42\n");
}

/// `alias` and `undef` at TOP LEVEL, where the implicit target is `Object` --
/// top-level `def` defines a private method on Object, so the keywords that
/// alias or remove one have to reopen the same class. Both were class-body-only
/// and reached the generic "unsupported syntax" rejection out there.
#[test]
fn top_level_alias_and_undef_target_object() {
    let result = run_ruby(
        r#"
        def greet
          "hi"
        end
        alias hello greet
        puts hello
        puts greet

        def doomed
          "here"
        end
        undef doomed
        puts respond_to?(:doomed, true)
        begin
          doomed
        rescue NameError
          puts "NameError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\nhi\nfalse\nNameError\n");
}

#[test]
fn unknown_and_missing_keywords_collect_all_names_with_no_method_suffix() {
    // CRuby reports every offending keyword (singular/plural) with no
    // `(in 'method')` suffix, for both direct calls and **hash forwarding.
    let result = run_ruby(
        r#"
        def take(a:); a; end
        def two(a:, b:); [a, b]; end
        def rest(a:, **opts); [a, opts]; end
        def try
          yield
        rescue ArgumentError => e
          puts e.message
        end
        try { take(**{a: 1, b: 2}) }
        try { two(**{a: 1, b: 2, c: 3, d: 4}) }
        try { two(**{a: 1}) }
        p rest(a: 1, x: 9, y: 8)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "unknown keyword: :b\n\
         unknown keywords: :c, :d\n\
         missing keyword: :b\n\
         [1, {x: 9, y: 8}]\n"
    );
}
