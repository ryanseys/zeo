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
fn alias_keyword_inside_a_class_eval_block_aliases_at_runtime() {
    // `alias new old` in a general (non-class-body) context -- here a module's
    // `class_eval` block, where `self` is the module -- desugars to a runtime
    // `alias_method`, the same path delegate.rb's `kernel.class_eval do alias
    // __raise__ raise end` needs.
    let result = run_ruby(
        r#"
        class Foo
          def greet = "hi"
        end
        Foo.class_eval do
          alias hello greet
        end
        puts Foo.new.hello
        puts Foo.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\nhi\n");
}

#[test]
fn a_constant_inside_class_of_an_object_resolves_in_its_singleton_methods() {
    // `class << obj; MAX = ...; def m; MAX; end; end` -- a constant on an
    // object's singleton class (tmpdir's `class << RANDOM`). zeo hoists the
    // constant to the enclosing lexical scope, where the singleton method
    // resolves it.
    let result = run_ruby(
        r#"
        module Holder
          GEN = Object.new
          class << GEN
            STEP = 6
            def emit = STEP * 7
          end
        end
        puts Holder::GEN.emit
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn an_alias_inside_class_self_is_a_class_method_alias() {
    // `alias new old` inside `class << self` aliases a SINGLETON method: it
    // must resolve against the class-method table and register `new` as a class
    // method, not an instance method.
    let result = run_ruby(
        r#"
        module M
          def self.original = 42
          class << self
            alias renamed original
          end
        end
        puts M.renamed
        puts M.respond_to?(:renamed)
        puts M.singleton_methods.include?(:renamed)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\ntrue\ntrue\n");
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

/// Runtime `Module#alias_method` (computed names, ostruct's bulk-`!` loop
/// shape): aliases of user methods, of builtins (`dup`/`class`), snapshot
/// semantics against a later runtime redefinition, the Symbol return value,
/// NameError for an unresolvable source, and the frozen-class refusal.
/// Expected output oracle-verified verbatim.
#[test]
fn runtime_alias_method_covers_user_and_builtin_sources() {
    let result = run_ruby(
        r##"
        class T
          def greet
            "hi"
          end
        end
        class T
          instance_methods(false).each do |m|
            alias_method "#{m}!", m
          end
        end
        t = T.new
        puts t.greet!
        class T
          ["dup", "class"].each { |m| alias_method "my_#{m}", m.to_sym }
        end
        p t.my_class
        p t.my_dup.class
        class Snap; end
        n1 = :v
        Snap.class_eval { define_method(n1) { "old" } }
        Snap.class_eval do
          [[:v2, :v]].each { |a, b| p alias_method(a, b) }
        end
        Snap.class_eval { define_method(n1) { "new" } }
        o = Snap.new
        p o.v2
        p o.v
        class T
          begin
            [[:x, :nope]].each { |a, b| alias_method(a, b) }
          rescue NameError => e
            puts e.message
          end
        end
        class Fz
          def m
            1
          end
        end
        Fz.freeze
        begin
          Fz.class_eval { [[:m2, :m]].each { |a, b| alias_method(a, b) } }
        rescue FrozenError => e
          puts e.message
        end
        puts "done"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hi\nT\nT\n:v2\n\"old\"\n\"new\"\nundefined method 'nope' for class 'T'\ncan't modify frozen Class: Fz\ndone\n"
    );
}

/// Runtime `Module#private`/`public`/`protected` with name arguments
/// (`class_eval { private :m }`): marks land in the overlay and govern the
/// DYNAMIC paths -- `respond_to?`, `public_send`, plain `send` -- with the
/// return shapes, NameError timing, and an alias inheriting its source's
/// runtime-marked visibility all oracle-verified verbatim. (Static call
/// sites with literal names resolve visibility at compile time and don't
/// see runtime marks -- the documented AOT boundary -- so every probe here
/// uses a computed name.)
#[test]
fn runtime_visibility_marks_govern_dynamic_dispatch() {
    let result = run_ruby(
        r#"
        class C
          def m
            1
          end
          def m2
            2
          end
        end
        p C.class_eval { private :m }
        p C.class_eval { private :m, :m2 }
        sym = :m
        p C.new.respond_to?(sym)
        begin
          C.new.public_send(sym)
        rescue NoMethodError => e
          puts e.message
        end
        p C.new.send(sym)
        p C.class_eval { public "m" }
        p C.new.respond_to?(sym)
        p C.new.public_send(sym)
        class D
          def p1
            3
          end
        end
        D.class_eval { protected :p1 }
        psym = :p1
        begin
          D.new.public_send(psym)
        rescue NoMethodError => e
          puts e.message
        end
        begin
          C.class_eval { private :nope }
        rescue NameError => e
          puts e.message
        end
        class Al
          def hidden
            4
          end
        end
        Al.class_eval { private :hidden }
        Al.class_eval { [[:hidden2, :hidden]].each { |a, b| alias_method(a, b) } }
        h2 = :hidden2
        begin
          Al.new.public_send(h2)
        rescue NoMethodError => e
          puts e.message
        end
        p Al.new.send(h2)
        puts "done"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        ":m\n[:m, :m2]\nfalse\nprivate method 'm' called for an instance of C\n1\n\"m\"\ntrue\n1\nprotected method 'p1' called for an instance of D\nundefined method 'nope' for class 'C'\nprivate method 'hidden2' called for an instance of Al\n4\ndone\n"
    );
}

/// `Ractor.make_shareable` on a Proc freezes it in place (returns the same
/// object) and `shareable?` reports the frozen state -- the ostruct
/// `new_ostruct_member!` shape (`nil.instance_eval { Proc.new { ... } }`
/// then `::Ractor.make_shareable(proc)`), where the `::Ractor` cpath form
/// exercises the runtime class-method rows rather than the static fold.
/// Oracle-verified. (zeo freezes without CRuby's isolation check -- see
/// ractor.rs -- so the IsolationError arm is deliberately not pinned.)
#[test]
fn ractor_make_shareable_freezes_a_proc_through_the_dynamic_rows() {
    let result = run_ruby(
        r#"
        name = :a
        pr = nil.instance_eval { Proc.new { name } }
        r = ::Ractor.make_shareable(pr)
        p r.equal?(pr)
        p pr.frozen?
        p ::Ractor.shareable?(pr)
        p ::Ractor.shareable?(proc { 1 })
        p ::Ractor.shareable?(:sym)
        p ::Ractor.shareable?("mut")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\nfalse\ntrue\nfalse\n");
}

/// `respond_to_missing?` is wired into the whole `respond_to?` surface
/// (the codegen folds and the Kernel row), `method(:dyn)` succeeds when the
/// hook admits the name (its Method dispatches through `method_missing`),
/// and `defined?(recv.dyn)` reports "method". The default `method_missing`/
/// `respond_to_missing?` rows are hidden-private (invisible to `methods`,
/// answer `respond_to?` only with `include_all`) and are what an override's
/// `super` reaches -- a bare `super` in `method_missing` raises the real
/// NoMethodError. Output oracle-verified verbatim.
#[test]
fn respond_to_missing_and_method_missing_protocol() {
    let result = run_ruby(
        r##"
        class R
          def respond_to_missing?(name, include_private = false)
            name.to_s.start_with?("dyn_") || super
          end
          def method_missing(name, *args)
            if name.to_s.start_with?("dyn_")
              "handled #{name}"
            else
              super
            end
          end
        end
        r = R.new
        p r.respond_to?(:dyn_foo)
        p r.respond_to?(:other)
        p r.dyn_foo
        p defined?(r.dyn_foo)
        p defined?(r.other)
        m = r.method(:dyn_bar)
        p m.call
        begin
          r.nope_at_all
        rescue NoMethodError => e
          puts e.message
        end
        o = Object.new
        p o.respond_to?(:method_missing)
        p o.respond_to?(:method_missing, true)
        p Object.new.methods.include?(:respond_to_missing?)
        puts "done"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\n\"handled dyn_foo\"\n\"method\"\nnil\n\"handled dyn_bar\"\nundefined method 'nope_at_all' for an instance of R\nfalse\ntrue\nfalse\ndone\n"
    );
}

/// The M8 eval-VM tail: `def` inside `eval`/`class_eval`/`instance_eval`
/// (installing on the right definee), optional+rest+keyword params, `return`
/// and `yield` in an eval-defined method, and blocks + auto-splat passed to
/// calls inside eval. Each source is non-literal (held in a variable / built
/// with a heredoc) so it runs through the runtime eval VM, not the inline
/// splice. Output oracle-verified verbatim.
#[test]
fn eval_vm_defines_methods_and_runs_blocks() {
    let result = run_ruby(
        r#"
        code = <<~RUBY
          def calc(a, b = 10, *rest)
            return a + b if rest.empty?
            a + b + rest.sum
          end
        RUBY
        eval(code)
        puts calc(1)
        puts calc(1, 2, 3, 4)
        eval("def each_twice; yield 1; yield 2; end")
        out = []
        each_twice { |n| out << n * 10 }
        p out
        class Widget; end
        Widget.class_eval("def name; \"widget\"; end")
        puts Widget.new.name
        obj = Object.new
        obj.instance_eval("def secret; 42; end")
        puts obj.secret
        p Object.new.respond_to?(:secret)
        eval("def kw(a, x:, y: 5); [a, x, y]; end")
        p kw(1, x: 2)
        begin; kw(1); rescue ArgumentError => e; puts e.message; end
        begin; calc; rescue ArgumentError => e; puts e.message; end
        p eval("[1, 2, 3].map { |x| x * 2 }")
        p eval("[[1, 2], [3, 4]].map { |a, b| a + b }")
        p eval("def foo; end")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "11\n10\n[10, 20]\nwidget\n42\nfalse\n[1, 2, 5]\nmissing keyword: :x\nwrong number of arguments (given 0, expected 1+)\n[2, 4, 6]\n[3, 7]\n:foo\n"
    );
}

/// `class`/`module` bodies inside eval: a fresh class with initialize+ivars,
/// a subclass whose method calls `super`, a module function, reopening a
/// compiled class, and the class expression's value. Non-literal source ->
/// the runtime eval VM. Oracle-verified verbatim.
#[test]
fn eval_vm_defines_classes_and_modules() {
    let result = run_ruby(
        r#"
        code = <<~RUBY
          class Point
            def initialize(x, y)
              @x = x
              @y = y
            end
            def sum
              @x + @y
            end
          end
        RUBY
        eval(code)
        p Point.new(3, 4).sum
        eval("class Base; def kind; \"base\"; end; end; class Sub < Base; def kind; \"sub:\" + super; end; end")
        p Sub.new.kind
        eval("module Helpers; def self.double(n); n * 2; end; end")
        p Helpers.double(21)
        class Widget
          def base; 1; end
        end
        eval("class Widget; def extra; base + 10; end; end")
        p Widget.new.extra
        p eval("class Empty; 99; end")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n\"sub:base\"\n42\n11\n99\n");
}

/// `define_method(name, method_obj)` / `define_singleton_method(name,
/// method_obj)` with a `Method`/`UnboundMethod` body (not a Proc): aliases
/// within a class, an UnboundMethod copied into a subclass, a bound Method
/// as an object singleton, the subclass-compatibility TypeError, and the
/// still-working Proc form. Oracle-verified verbatim.
#[test]
fn define_method_accepts_a_method_object_body() {
    let result = run_ruby(
        r#"
        class A
          def greet(n); "hi #{n}"; end
          r = define_method(:hail, instance_method(:greet))
          p r
        end
        p A.new.hail("x")
        class Sub < A
          define_method(:hey, A.instance_method(:greet))
        end
        p Sub.new.hey("y")
        class B < A
          def m(n); n * 2; end
        end
        class C < B
          define_method(:m2, B.instance_method(:m))
        end
        p C.new.m2(5)
        class Unrelated; end
        begin
          Unrelated.class_eval { define_method(:g, A.instance_method(:greet)) }
        rescue TypeError => e
          puts e.message
        end
        class Widget
          def ping(x); "pong #{x}"; end
        end
        w = Widget.new
        w.define_singleton_method(:sm, Widget.new.method(:ping))
        p w.sm("z")
        class D
          define_method(:sq) { |x| x * x }
        end
        p D.new.sq(6)
        puts "done"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        ":hail\n\"hi x\"\n\"hi y\"\n10\nbind argument must be a subclass of A\n\"pong z\"\n36\ndone\n"
    );
}

#[test]
fn a_conditionally_defined_method_is_visible_to_reflection_and_extend() {
    // A `def` nested in a `case`/`if` branch (a platform-conditional definition,
    // as in fileutils' StreamUtils_) is registered as an own method, so
    // `instance_methods` and `extend` -- both resolved at compile time in zeo --
    // can see it. A branch the compiler folds away (`if false`) still defines
    // nothing, matching CRuby.
    let result = run_ruby(
        r#"
        module Inner
          case 1
          when 2 then def flavor; "two"; end
          else def flavor; "other"; end
          end
          if false
            def gated; "on"; end
          end
        end
        p Inner.instance_methods(false).sort
        module Host
          extend Inner
          puts flavor
        end
        p Host.respond_to?(:gated)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[:flavor]\nother\nfalse\n");
}
