use crate::support::{compile_project, run_ruby, run_ruby_project};

#[test]
fn defined_classifies_syntactic_form() {
    let result = run_ruby(
        r#"
        x = 5
        puts(defined?(x))
        puts(defined?(1 + 1))

        class Foo
          def check
            @bar = 1
            puts(defined?(@bar))
          end
        end
        Foo.new.check
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "local-variable\nmethod\ninstance-variable\n");
}

#[test]
fn class_new_creates_an_anonymous_runtime_class() {
    // `Class.new(Super) { ... }` mints a runtime class. The block body
    // runs against the new class (`define_method` and `def` both install on
    // it); a constant binding names it; is_a?/instance_of?/superclass and a
    // runtime superclass chain all resolve.
    let result = run_ruby(
        r#"
        Widget = Class.new do
          define_method(:kind) { "widget" }
          def size
            10
          end
        end
        w = Widget.new
        puts w.kind
        puts w.size
        puts w.is_a?(Widget)
        puts Widget.name
        Gadget = Class.new(Widget) do
          define_method(:extra) { "gadget" }
        end
        g = Gadget.new
        puts g.kind
        puts g.extra
        puts g.is_a?(Widget)
        puts g.instance_of?(Widget)
        puts Gadget.superclass.name
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "widget\n10\ntrue\nWidget\nwidget\ngadget\ntrue\nfalse\nWidget\n"
    );
}

// --- Full MRO (include/extend/prepend), inherited ivars, class
// variables, minimal raise/exception foundation -- oracle-verified against
// real `ruby` first, per this project's established convention.

#[test]
fn a_subclass_calls_a_non_overridden_inherited_method() {
    // Closes a real, pre-existing latent gap: a class's own LITERAL methods
    // each get a generated Rust method, but `Dog.new.speak` with no override
    // at all has none to call (Rust has no cross-struct inherent-method
    // inheritance). `analyze::mro`'s
    // materialization fixes this as a byproduct of doing modules correctly
    // (every MRO-reachable method gets its own Scope on the receiver).
    let result = run_ruby(
        r#"
        class Animal
          def speak
            "generic"
          end
        end
        class Dog < Animal
        end
        puts Dog.new.speak
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "generic\n");
}

#[test]
fn a_class_own_method_shadows_an_included_module_method() {
    let result = run_ruby(
        r#"
        module Greetable
          def greet
            "hi"
          end
        end
        class Person
          include Greetable
          def greet
            "overridden"
          end
        end
        class Robot
          include Greetable
        end
        puts Person.new.greet
        puts Robot.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "overridden\nhi\n");
}

#[test]
fn stacked_prepends_resolve_in_reverse_declaration_order() {
    // `prepend M1; prepend M2` -- M2 (most recently prepended) is closest,
    // ahead of M1, ahead of the class's own definition.
    let result = run_ruby(
        r#"
        module M1
          def label
            "m1"
          end
        end
        module M2
          def label
            "m2"
          end
        end
        class Stacked
          prepend M1
          prepend M2
          def label
            "own"
          end
        end
        puts Stacked.new.label
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "m2\n");
}

#[test]
fn a_module_of_module_diamond_dispatches_and_is_a_resolves_transitively() {
    // The exact shape zeo's OWN reflection gets wrong (verified by
    // running a repro against zeo's own binary): `D` included via two
    // separate paths (`B`/`C`, both including `D`) must be deduped to a
    // single shared position, and `is_a?(D)` must resolve `true` even
    // though `D` was never included DIRECTLY by `A`.
    let result = run_ruby(
        r#"
        module D
          def who
            "D"
          end
        end
        module B
          include D
        end
        module C
          include D
        end
        class A
          include B
          include C
        end
        class Unrelated
        end
        puts A.new.who
        puts A.new.is_a?(D)
        puts A.new.is_a?(B)
        puts A.new.is_a?(Unrelated)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "D\ntrue\ntrue\nfalse\n");
}

#[test]
fn extend_pulls_in_module_instance_methods_as_class_methods() {
    let result = run_ruby(
        r#"
        module MathHelpers
          def double(x)
            x * 2
          end
        end
        class Calc
          extend MathHelpers
        end
        puts Calc.double(21)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn class_methods_are_inherited_by_a_subclass_with_no_override() {
    let result = run_ruby(
        r#"
        class Base
          def self.bump
            1
          end
        end
        class Sub < Base
        end
        puts Sub.bump
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn prepend_and_include_together_resolve_in_correct_precedence_order() {
    // MRO here is `Pre, Own, Inc, Object` -- `Own`'s own `label` calls
    // `super` and reaches `Inc` (the next ancestor after `Own`); `Pre`'s
    // `label` calls `super` and reaches `Own`.
    let result = run_ruby(
        r#"
        module Pre
          def label
            "pre(#{super})"
          end
        end
        module Inc
          def label
            "inc"
          end
        end
        class Own
          prepend Pre
          include Inc
          def label
            "own(#{super})"
          end
        end
        puts Own.new.label
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "pre(own(inc))\n");
}

#[test]
fn extend_and_include_can_be_combined_on_the_same_class() {
    let result = run_ruby(
        r#"
        module Ext
          def helper(x)
            x + 1
          end
        end
        module Inc
          def instance_helper
            "inc"
          end
        end
        class Both
          extend Ext
          include Inc
        end
        puts Both.helper(9)
        puts Both.new.instance_helper
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\ninc\n");
}

#[test]
fn reopening_object_dispatches_to_builtin_and_user_receivers() {
    // reopening Object (with an include + a def) makes those methods
    // dispatch on built-in AND user receivers with the real receiver as self.
    let result = run_ruby(
        r#"
        module ObjectGreeting
          def hi
            "hi from " + self.class.name
          end
        end
        class Object
          include ObjectGreeting
          def global_hi
            "global " + self.class.name
          end
        end
        class LocalThing
        end
        puts "x".hi
        puts "x".global_hi
        puts 1.global_hi
        puts [1, 2].global_hi
        puts LocalThing.new.global_hi
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hi from String\nglobal String\nglobal Integer\nglobal Array\nglobal LocalThing\n"
    );
}

/// A user subclass of `Array` (D3) is the native `ValueSubclass` wrapping an
/// array payload: inherited methods (`push`/`<<`/`size`/`each`/`map`) run
/// against it, self-returning mutators re-wrap to the subclass while a NEW
/// collection (`map`) is a plain `Array`, and a custom method sees the elements.
#[test]
fn value_subclass_of_array() {
    let result = run_ruby(
        r#"
        class Stack < Array
          def peek; last; end
        end
        s = Stack.new
        s.push(1)
        s << 2
        p s
        puts s.size
        puts s.peek
        puts s.class
        puts s.push(3).class
        puts s.map { |x| x * 2 }.inspect
        puts s.map { |x| x }.class
        puts s.is_a?(Array)
        puts Stack.new([9, 8]).size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n2\n2\nStack\nStack\n[2, 4, 6]\nArray\ntrue\n2\n"
    );
}

/// A `String` subclass (D3): inherited `String` methods, a `super`-free custom
/// method, `dup` independence, and payload-based equality/`Hash`-key identity
/// (`Tag.new("k")` is the same key as `"k"`, symmetric `==`).
#[test]
fn value_subclass_of_string_equality_and_hashing() {
    let result = run_ruby(
        r#"
        class Tag < String
          def shout; upcase + "!"; end
        end
        t = Tag.new("hi")
        puts t.shout
        puts t.class
        puts(t == "hi")
        puts("hi" == t)
        puts(Tag.new("x").hash == "x".hash)
        h = { "key" => 1 }
        puts h[Tag.new("key")].inspect
        d = Tag.new("a")
        d2 = d.dup
        d2 << "b"
        puts d
        puts d2
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "HI!\nTag\ntrue\ntrue\ntrue\n1\na\nab\n");
}

/// A `Numeric` subclass (D3) is an ordinary ivar-carrying object: it defines its
/// own `<=>`/state, and `Comparable` (inherited through `Numeric`) drives
/// `<`/`>`/`min` off that `<=>`.
#[test]
fn numeric_subclass_is_a_plain_comparable_object() {
    let result = run_ruby(
        r#"
        class Money < Numeric
          def initialize(cents); @cents = cents; end
          def cents; @cents; end
          def <=>(o); cents <=> o.cents; end
        end
        m = Money.new(500)
        n = Money.new(300)
        puts m.cents
        puts(m > n)
        puts(m == Money.new(500))
        puts m.is_a?(Numeric)
        puts m.is_a?(Comparable)
        puts m.class
        puts [m, n].min.cents
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "500\ntrue\ntrue\ntrue\ntrue\nMoney\n300\n");
}

/// Reopening a builtin MODULE (D3): the added method reaches every includer --
/// Enumerable across Array/Hash, Comparable across Integer.
#[test]
fn builtin_module_reopen_reaches_all_includers() {
    let result = run_ruby(
        r#"
        module Enumerable
          def second
            first(2).last
          end
        end
        module Comparable
          def clamp_low(lo)
            self < lo ? lo : self
          end
        end
        puts [10, 20, 30].second
        puts({ a: 1, b: 2 }.map { |k, v| v }.second)
        puts 5.clamp_low(8)
        puts 12.clamp_low(8)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n2\n8\n12\n");
}

#[test]
fn built_in_hierarchy_extension_classes_are_real_and_ancestor_matched() {
    // `KeyError < IndexError` -- exercises the extended built-in hierarchy
    // (an addition beyond the original minimal foundation).
    let result = run_ruby(
        r#"
        begin
          raise KeyError, "missing"
        rescue IndexError => e
          puts "caught KeyError via IndexError: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught KeyError via IndexError: missing\n");
}

#[test]
fn defined_on_a_begin_expression_classifies_as_expression() {
    let result = run_ruby("puts defined?(begin; 1; end)\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "expression\n");
}

#[test]
fn self_inside_a_class_method_is_the_class_object() {
    // A class method's `self` is the class object (a `RubyValue::Class`
    // value), so `self` and the class constant are interchangeable --
    // including as a receiver for the class's OWN other class methods.
    let result = run_ruby(
        r#"
        class Foo
          def self.bar
            self
          end
          def self.baz
            self.bar.name
          end
        end
        p Foo.bar
        p Foo.bar == Foo
        p Foo.baz
        p Foo.bar.new.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Foo\ntrue\n\"Foo\"\nFoo\n");
}

#[test]
fn a_class_method_dispatches_dynamically_through_a_class_valued_variable() {
    // The receiver isn't a literal constant, so codegen can't emit a direct
    // `H1::__cm_run(...)` -- it goes through the registry's class-method
    // table on a `RubyValue::Class` receiver.
    let result = run_ruby(
        r#"
        class H1
          def self.run(x); "h1:#{x}"; end
        end
        class H2
          def self.run(x); "h2:#{x}"; end
        end
        [H1, H2].each { |h| puts h.run(5) }
        handler = H1
        puts handler.run(9)
        handler = H2
        puts handler.run(9)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "h1:5\nh2:5\nh1:9\nh2:9\n");
}

#[test]
fn an_implicit_self_call_in_a_class_method_resolves_against_the_receiver() {
    // `self` in a class method is the class it was CALLED on, not the one
    // whose body the method was written in. Both of these were silently wrong
    // (no error, just the wrong object) while implicit-self resolution used
    // the lexical `defining_class`:
    //   - `Sub.create` built a Base;
    //   - `Ext.helped` answered "Helper".
    let result = run_ruby(
        r#"
        class Base
          def self.create; new; end
          def self.who; name; end
        end
        class Sub < Base; end
        p Base.create.class
        p Sub.create.class
        p Sub.who

        module Helper
          def helped; "helped-#{name}"; end
        end
        class Ext
          extend Helper
        end
        p Ext.helped
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Base\nSub\n\"Sub\"\n\"helped-Ext\"\n");
}

#[test]
fn protected_method_callable_from_a_related_classs_own_method() {
    let result = run_ruby(
        r#"
        class Money
          def initialize(amount)
            @amount = amount
          end

          def greater_than_five
            other = Money.new(5)
            amount > other.amount
          end

          protected

          def amount
            @amount
          end
        end

        puts Money.new(10).greater_than_five
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn matchdata_offset_names_regexp_and_regexp_class_methods() {
    let result = run_ruby(
        r#"
        m = "abc123".match(/([a-z]+)(\d+)/)
        p m.offset(2)
        p m.byteoffset(2)
        p "x1".match(/(?<a>[a-z])(?<b>\d)/).names
        p Regexp.union("a", "b").source
        p Regexp.union.source
        p Regexp.linear_time?(/(a+)+/)
        p Regexp.linear_time?(/(a)\1/)
        p Regexp.try_convert("x")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[3, 6]\n[3, 6]\n[\"a\", \"b\"]\n\"a|b\"\n\"(?!)\"\ntrue\nfalse\nnil\n"
    );
}

#[test]
fn set_subtract_flatten_map_filter_classify_and_divide() {
    let result = run_ruby(
        r#"
        require "set"
        s = Set[1, 2, 3, 4]
        p s.subtract([2, 3]).to_a.sort
        p s.replace([9, 8, 8, 7]).to_a.sort
        p Set[Set[1, 2], Set[3, Set[4]]].flatten.to_a.sort
        p Set[Set[1, 2], Set[3]].flatten!.to_a.sort
        p Set[1, 2].flatten!
        m = Set[1, 2, 3]; m.map! { |x| x * 10 }; p m.to_a.sort
        p Set[1, 2, 3, 4].select! { |x| x.even? }.to_a.sort
        p Set[2, 4].select! { |x| x.even? }
        p Set[1, 2, 3, 4].reject! { |x| x.even? }.to_a.sort
        p Set[1, 2, 3, 4, 5].classify { |x| x % 3 }.transform_values { |v| v.to_a.sort }
        p Set[1, 2, 3, 4].divide { |i| i % 3 }.map { |g| g.to_a.sort }.sort
        p Set[1, 2, 3, 4].divide { |x, y| (x - y).abs == 1 }.map { |g| g.to_a.sort }.sort
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 4]\n[7, 8, 9]\n[1, 2, 3, 4]\n[1, 2, 3]\nnil\n[10, 20, 30]\n[2, 4]\nnil\n\
         [1, 3]\n{1 => [1, 4], 2 => [2, 5], 0 => [3]}\n[[1, 4], [2], [3]]\n[[1, 2, 3, 4]]\n"
    );
}

#[test]
fn extended_mode_ignores_whitespace_and_comments() {
    let result = run_ruby(
        r#"
        r = /
          \d+  # a number
          -
          \d+  # another number
        /x
        puts r.match?("123-456")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn class_shift_self_works_inside_a_module() {
    let result = run_ruby(
        r#"
        module MyMath
          class << self
            def double(x)
              x * 2
            end
          end
        end
        puts MyMath.double(5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n");
}

#[test]
fn class_shift_self_include_extends_the_enclosing_class() {
    // `include M` inside `class << self` == `extend M` on the enclosing class:
    // the module's instance methods become the class's class methods.
    let result = run_ruby(
        r##"
        module Greeter
          def hi(n); "hi #{n}"; end
        end
        class Widget
          class << self
            include Greeter
          end
        end
        puts Widget.hi("bob")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi bob\n");
}

// --- Bugs found via a comprehensive sweep (fixed, not deferred) ---
//
// Found by testing constructs adjacent to earlier work, not
// by design review -- each is a real, previously-undetected defect, not a
// documented scope-cut. Every test here was run against real `ruby` first,
// per this project's established convention.

#[test]
fn implicit_self_call_between_sibling_class_methods_dispatches_correctly() {
    // Found while testing `class << self`: `current_class` is
    // `None` inside a class method's own `Ctx` (no concrete `self` receiver
    // exists there), so a no-receiver call to a SIBLING class method
    // (`def self.a; b; end` calling `def self.b`) always panicked --
    // `codegen::call::emit_call`'s implicit-self branch only ever consulted
    // `current_class`. Fixed by also checking `defining_class` against
    // `Compiler::class_method_in_chain`, dispatching as a direct
    // associated-function call, same as `ClassName.foo(...)`.
    let result = run_ruby(
        r#"
        class Widget
          def self.create
            helper
          end
          def self.helper
            "made"
          end
        end
        puts Widget.create

        class MathUtils
          class << self
            def sum_of_squares(a, b)
              square(a) + square(b)
            end
            def square(x)
              x * x
            end
          end
        end
        puts MathUtils.sum_of_squares(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "made\n25\n");
}

#[test]
fn a_module_function_can_call_another_modules_function() {
    // Pre-existing gap found by 14.2's cross-package test: module-function
    // bodies live inside a generated `pub mod` (a real child module), so a
    // reference to a sibling top-level module didn't resolve. One file, no
    // packages needed.
    let result = run_ruby(
        r#"
            module A
              def self.f
                41
              end
            end
            module B
              def self.g
                A.f + 1
              end
            end
            puts B.g
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn digest_class_and_streaming_apis_match_ruby() {
    let result = run_ruby(
        r#"
        require "digest"
        puts Digest::SHA256.hexdigest("abc")
        puts Digest::MD5.hexdigest("")
        d = Digest::SHA256.new
        d << "a"; d.update("bc")
        puts d.hexdigest
        puts Digest::SHA256.base64digest("abc")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
         d41d8cd98f00b204e9800998ecf8427e\n\
         ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
         ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=\n"
    );
}

#[test]
fn reopening_enumerable_reaches_every_includer() {
    // Enumerable is a RUST-implemented builtin, yet reopenable (D3): an added
    // method registers as a value method on the module id and the MRO walk
    // finds it for every includer, its body free to drive the native
    // Enumerable protocol (`reduce`) on the receiver.
    let result = run_ruby(
        r#"
        module Enumerable
          def my_join
            reduce("") { |acc, x| acc + x.to_s }
          end
        end
        puts [1, 2, 3].my_join
        puts (1..3).my_join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "123\n123\n");
}

// -- Correctness fixes (reopening, bare-super forwarding, cycle
// guards, dup/clone). Every positive expectation below is oracle-verified
// against real ruby 4.0.5.

#[test]
fn reopening_a_user_class_adds_and_replaces_methods() {
    // Pre-15.2 this was the codebase's one SILENT-wrongness bug: the second
    // `class Foo` registered a shadowed duplicate, so `b` never dispatched
    // and the original `a` kept winning.
    let result = run_ruby(
        r#"
        class Foo
          def a
            "first"
          end
        end

        class Foo
          def b
            "added"
          end

          def a
            "replaced"
          end
        end

        puts Foo.new.a
        puts Foo.new.b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "replaced\nadded\n");
}

#[test]
fn reopening_via_load_twice_merges_cleanly() {
    let result = run_ruby_project(
        &[
            (
                "counter.rb",
                r#"
                    class Counter
                      def bump
                        1
                      end
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    load "./counter.rb"
                    load "./counter.rb"
                    puts Counter.new.bump
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn same_leaf_class_name_in_two_namespaces_stays_distinct() {
    let result = run_ruby(
        r#"
        module A1
          class Widget
            def tag
              "a"
            end
          end
        end

        module B1
          class Widget
            def tag
              "b"
            end
          end
        end

        puts A1::Widget.new.tag
        puts B1::Widget.new.tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n");
}

#[test]
fn including_a_nested_module_materializes_its_methods() {
    let result = run_ruby(
        r#"
        module Util
          module Greet
            def hello
              "hello from nested module"
            end
          end
        end

        class Greeter2
          include Util::Greet
        end

        puts Greeter2.new.hello
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello from nested module\n");
}

// -- First-class Class/Module values. Every expectation
// oracle-verified against real ruby 4.0.5.

#[test]
fn class_values_print_and_compare_by_identity() {
    let result = run_ruby(
        r#"
        class Widget
        end

        module Store
          class Item
          end
        end

        puts Widget
        p Widget
        puts Store::Item
        puts Widget == Widget
        puts Widget == Store::Item
        puts Widget != Store::Item
        arr = [Widget, Store::Item]
        puts arr.include?(Widget)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Widget\nWidget\nStore::Item\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn dot_class_reflects_every_receiver_kind() {
    let result = run_ruby(
        r#"
        class Widget
        end
        module Helper
        end

        w = Widget.new
        puts w.class
        puts w.class == Widget
        puts w.class.name
        puts 5.class
        puts "s".class
        puts [].class
        puts nil.class
        puts true.class
        puts 1.5.class
        puts :sym.class
        puts (1..2).class
        puts({}.class)
        puts Helper.class
        puts Widget.class
        puts Class.class
        puts Widget.class == Class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Widget\ntrue\nWidget\nInteger\nString\nArray\nNilClass\nTrueClass\nFloat\nSymbol\nRange\nHash\nModule\nClass\nClass\ntrue\n"
    );
}

#[test]
fn is_a_and_instance_of_answer_through_class_values() {
    let result = run_ruby(
        r#"
        class Widget
        end
        module Helper
        end

        w = Widget.new
        puts w.instance_of?(Widget)
        puts w.instance_of?(Object)
        puts 5.instance_of?(Integer)
        puts Widget.is_a?(Class)
        puts Widget.is_a?(Module)
        puts Helper.is_a?(Module)
        puts Helper.is_a?(Class)
        puts Widget.ancestors.first == Widget
        puts Widget.ancestors.include?(Object)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\ntrue\ntrue\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn comparable_drives_the_includers_spaceship() {
    let result = run_ruby(
        r#"
        class Temp
          include Comparable
          attr_reader :deg

          def initialize(d)
            @deg = d
          end

          def <=>(other)
            deg <=> other.deg
          end
        end

        a = Temp.new(50)
        b = Temp.new(70)
        puts a < b
        puts a > b
        puts a <= b
        puts b >= a
        puts a == Temp.new(50)
        puts a.between?(Temp.new(40), Temp.new(60))
        puts a.clamp(Temp.new(55), Temp.new(80)).deg
        puts a.clamp(Temp.new(20), Temp.new(30)).deg
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\ntrue\ntrue\ntrue\n55\n30\n"
    );
}

#[test]
fn explicit_triple_equals_on_class_values_checks_ancestry() {
    let result = run_ruby(
        r#"
        class Widget
        end

        puts Integer === 5
        puts Widget === Widget.new
        puts Widget === 5
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\n");
}

// ---------------------------------------------------------------------------
// Builtin-class reopening (root). Every expectation below was
// oracle-verified against real `ruby` (4.0.5) before being written down.
// ---------------------------------------------------------------------------

/// The Box docs' motivating example shape: a fresh method on `class String`,
/// dispatching statically ("".blank?), dynamically through a Poly ivar
/// (@s.blank?), and calling a NATIVE builtin method (`length`) implicitly on
/// self from inside the reopen.
#[test]
fn builtin_reopen_string_blank_dispatches_everywhere() {
    let result = run_ruby(
        r#"
        class String
          def blank?
            length == 0
          end
        end

        class Foo
          def initialize(s)
            @s = s
          end

          def foo_is_blank?
            @s.blank?
          end
        end

        puts "".blank?
        puts "  hi".blank?
        puts Foo.new("").foo_is_blank?
        puts Foo.new("x").foo_is_blank?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\nfalse\n");
}

/// Overriding an EXISTING native method: the user `length` wins at a static
/// call site (where `try_collection_dispatch` would answer 3), at a dynamic
/// Poly site, and `size` -- a separate method in real Ruby, NOT an alias of
/// the override -- stays native.
#[test]
fn builtin_reopen_override_beats_native_length() {
    let result = run_ruby(
        r#"
        class String
          def length
            42
          end
        end

        s = "abc"
        puts s.length
        puts [s].first.length
        puts "xy".size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n42\n2\n");
}

/// Blocks through a builtin reopen: `yield` + an optional parameter, and an
/// escaping block that captures both a mutated local (a Captured cell) and
/// `self` (the `__self: RubyValue` clone path).
#[test]
fn builtin_reopen_methods_take_blocks_and_capture_self() {
    let result = run_ruby(
        r#"
        class Integer
          def repeat(sep = "-")
            out = ""
            i = 0
            while i < self
              out = out + yield(i).to_s
              out = out + sep if i < self - 1
              i = i + 1
            end
            out
          end

          def add_each(arr)
            total = 0
            arr.each do |x|
              total = total + x + self
            end
            total
          end
        end

        puts 3.repeat { |i| i * 2 }
        puts 2.repeat("+") { |i| i + 1 }
        puts 10.add_each([1, 2, 3])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0-2-4\n1+2\n36\n");
}

/// A patch calling another patch (implicit self -> direct free-fn call, the
/// result's `+` going back through dynamic String dispatch), and an unknown
/// method still raising real Ruby's exact NoMethodError.
#[test]
fn builtin_reopen_chained_patches_and_nme() {
    let result = run_ruby(
        r#"
        class String
          def shout
            exclaim + "?"
          end

          def exclaim
            self + "!"
          end
        end

        puts "hey".shout
        begin
          "hey".nope
        rescue NoMethodError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hey!?\nundefined method 'nope' for an instance of String\n"
    );
}

/// The receiver-kind breadth: NilClass (no static TyKind -- fully dynamic),
/// Hash (implicit native `size`), and Range (static `self.last`/`self.first`
/// fast paths inside the reopen body).
#[test]
fn builtin_reopen_nil_hash_and_range() {
    let result = run_ruby(
        r#"
        class NilClass
          def describe
            "nothing"
          end
        end

        class Hash
          def pair_count
            size
          end
        end

        class Range
          def span
            self.last - self.first
          end
        end

        puts nil.describe
        puts({ a: 1, b: 2 }.pair_count)
        puts (3..9).span
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nothing\n2\n6\n");
}

/// `to_s`/`inspect` overrides on a builtin drive `puts`, interpolation,
/// `p`, CONTAINER inspect (real Ruby's rb_inspect dispatches per element --
/// oracle-verified `[5].inspect` -> `[I]`), and explicit `.to_s`.
#[test]
fn builtin_reopen_to_s_and_inspect_drive_rendering() {
    let result = run_ruby(
        r#"
        class Integer
          def to_s
            "int"
          end

          def inspect
            "I"
          end
        end

        puts 5
        puts "v=#{5}"
        p 7
        puts [5].inspect
        puts 6.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "int\nv=int\nI\n[I]\nint\n");
}

/// A `dup` override beats universal `Kernel#dup` on both the static String
/// site and a Poly site (the any-builtin-overrides fall-through); `clone`
/// -- not overridden -- stays the universal shallow copy.
#[test]
fn builtin_reopen_dup_override_beats_kernel_dup() {
    let result = run_ruby(
        r#"
        class String
          def dup
            "dupped"
          end
        end

        s = "orig"
        puts s.dup
        puts [s].first.dup
        puts s.clone
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "dupped\ndupped\norig\n");
}

/// `send` with a literal symbol reaches a reopen method through
/// `send_value`'s value-method-first probe (the generated arity-checked
/// trampoline), same result as the direct static call.
#[test]
fn builtin_reopen_methods_reachable_via_send() {
    let result = run_ruby(
        r#"
        class String
          def echo(a, b)
            a.to_s + b.to_s + self
          end
        end

        puts "s".send(:echo, 1, 2)
        puts "s".echo(9, 8)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "12s\n98s\n");
}

// ---------------------------------------------------------------------------
// The CRuby-exact builtin hierarchy (BasicObject/Kernel/
// Numeric/Rational/Complex/Math/Struct/Enumerator in the ABI; declarative
// superclass/includes seeding). Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// THE keystone parity test: `.ancestors` for every core class, byte-
/// identical to real ruby. (Mutex/Queue excluded: real Ruby names them
/// `Thread::Mutex`/`Thread::Queue` -- a documented naming divergence.)
#[test]
fn builtin_ancestors_are_cruby_exact() {
    let result = run_ruby(
        r##"
        [BasicObject, Object, Kernel, Comparable, Enumerable,
         Numeric, Integer, Float, Rational, Complex,
         String, Symbol, Array, Hash, Range,
         NilClass, TrueClass, FalseClass,
         Proc, Regexp, MatchData, Struct, Enumerator,
         Class, Module, Math, Fiber, Thread, Ractor].each do |c|
          puts "#{c}: #{c.ancestors.inspect}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "BasicObject: [BasicObject]\n\
         Object: [Object, Kernel, BasicObject]\n\
         Kernel: [Kernel]\n\
         Comparable: [Comparable]\n\
         Enumerable: [Enumerable]\n\
         Numeric: [Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Integer: [Integer, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Float: [Float, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Rational: [Rational, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Complex: [Complex, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         String: [String, Comparable, Object, Kernel, BasicObject]\n\
         Symbol: [Symbol, Comparable, Object, Kernel, BasicObject]\n\
         Array: [Array, Enumerable, Object, Kernel, BasicObject]\n\
         Hash: [Hash, Enumerable, Object, Kernel, BasicObject]\n\
         Range: [Range, Enumerable, Object, Kernel, BasicObject]\n\
         NilClass: [NilClass, Object, Kernel, BasicObject]\n\
         TrueClass: [TrueClass, Object, Kernel, BasicObject]\n\
         FalseClass: [FalseClass, Object, Kernel, BasicObject]\n\
         Proc: [Proc, Object, Kernel, BasicObject]\n\
         Regexp: [Regexp, Object, Kernel, BasicObject]\n\
         MatchData: [MatchData, Object, Kernel, BasicObject]\n\
         Struct: [Struct, Enumerable, Object, Kernel, BasicObject]\n\
         Enumerator: [Enumerator, Enumerable, Object, Kernel, BasicObject]\n\
         Class: [Class, Module, Object, Kernel, BasicObject]\n\
         Module: [Module, Object, Kernel, BasicObject]\n\
         Math: [Math]\n\
         Fiber: [Fiber, Object, Kernel, BasicObject]\n\
         Thread: [Thread, Object, Kernel, BasicObject]\n\
         Ractor: [Ractor, Object, Kernel, BasicObject]\n"
    );
}

/// A `class Numeric` reopen materializes onto Integer AND Float (the mro
/// machinery) and is found on both static receivers and dynamic Poly ones
/// (the per-ancestor value-method walk).
#[test]
fn numeric_reopen_resolves_down_the_mro() {
    let result = run_ruby(
        r#"
        class Numeric
          def double
            self * 2
          end
        end

        puts 5.double
        puts 2.5.double
        puts [7].first.double
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n5.0\n14\n");
}

/// Complex: imaginary literals, component-class preservation (exact
/// Integer/Rational components incl. the den==1 demotion inside complex
/// division), formatting (`(2/25)*i`), polar surface, and coercion errors.
#[test]
fn complexes_keep_component_classes() {
    let result = run_ruby(
        r##"
        c = 4i
        puts c.class
        p c
        p 3 + 4i
        p Complex(1, 2) * Complex(3, 4)
        p Complex(1, 2) / Complex(3, 4)
        p Complex(1, 2) / 2
        p Complex(1, 2) ** 2
        p Complex(1.5, -2.5)
        puts Complex(1.5, -2.5)
        puts Complex(3, 4).abs
        p Complex(3, 4).abs2
        p Complex(3, 4).rect
        p Complex(1, -2).conjugate
        p Complex(2, 0) == 2
        p Complex(1, 2).real
        p Complex(1, 2).imaginary
        p 5.to_c
        begin
          Complex(1, 2) + "x"
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Complex\n(0+4i)\n(3+4i)\n(-5+10i)\n((11/25)+(2/25)*i)\n((1/2)+1i)\n(-3+4i)\n\
         (1.5-2.5i)\n1.5-2.5i\n5.0\n25\n[3, 4]\n(1+2i)\ntrue\n1\n2\n(5+0i)\n\
         TypeError: String can't be coerced into Complex\n"
    );
}

/// `Struct.new` in ANY position mints a native struct class at runtime: an
/// anonymous local/inline struct is a runtime value, exactly like the
/// constant form -- there is no compile-time-synthesized path.
#[test]
fn anonymous_struct_mints_a_native_class_at_runtime() {
    let result = run_ruby(
        r#"
        k = Struct.new(:a, :b)
        o = k.new(1, 2)
        p o
        p o.a
        p o.to_a
        p o.members
        o.a = 9
        p o.a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<struct a=1, b=2>\n1\n[1, 2]\n[:a, :b]\n9\n"
    );
}

/// `to_enum`/`enum_for` on a user class -- the real-Ruby
/// `return to_enum(:each) unless block_given?` pattern -- and the
/// synthesized Struct `each` using exactly that pattern.
#[test]
fn to_enum_works_on_user_classes_and_structs() {
    let result = run_ruby(
        r##"
        class Deck
          include Enumerable
          def initialize(cards)
            @cards = cards
          end
          def each
            return to_enum(:each) unless block_given?
            i = 0
            while i < @cards.length
              yield @cards[i]
              i += 1
            end
            self
          end
        end
        d = Deck.new([5, 3, 9])
        e = d.each
        p e.class
        p e.next
        p d.each.sort
        p d.map.with_index { |c, i| "#{i}:#{c}" }
        p 7.to_enum(:upto, 9).to_a
        P17 = Struct.new(:x, :y)
        pt = P17.new(1, 2)
        en = pt.each
        p en.class
        p en.next
        p en.to_a
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Enumerator\n5\n[3, 5, 9]\n[\"0:5\", \"1:3\", \"2:9\"]\n[7, 8, 9]\n\
         Enumerator\n1\n[1, 2]\n"
    );
}

#[test]
fn new_on_a_class_with_no_initialize_rejects_arguments() {
    // `Object#initialize` takes none, so passing any is an ArgumentError --
    // it was silently DROPPING them and constructing happily. Rescuable and
    // raised at runtime (plan G1), covering both the static `.new` path and
    // the dynamic one through a class-valued variable.
    let result = run_ruby(
        r#"
        class Bare; end
        begin
          Bare.new(1, 2)
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        k = Bare
        begin
          k.new(9)
        rescue ArgumentError => e
          puts "dyn: #{e.message}"
        end
        p Bare.new.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: wrong number of arguments (given 2, expected 0)\n\
         dyn: wrong number of arguments (given 1, expected 0)\nBare\n"
    );
}

#[test]
fn a_class_named_after_a_rust_prelude_type_compiles_and_behaves() {
    // `class Vec` would otherwise define a crate-root `Vec` struct shadowing
    // Rust's own, breaking every `Vec<RubyValue>` in the generated program.
    let result = run_ruby(
        r##"
        class Vec
          def initialize(x, y)
            @x = x
            @y = y
          end
          attr_reader :x, :y
          def +(other) = Vec.new(@x + other.x, @y + other.y)
          def to_s = "Vec(#{@x}, #{@y})"
        end
        class Option
          def initialize(v) = @v = v
          def some? = !@v.nil?
        end
        class Box
          def initialize(v) = @v = v
          def get = @v
        end
        puts(Vec.new(1, 2) + Vec.new(10, 20))
        p Vec.new(1, 2).class.name
        p Option.new(nil).some?
        p Box.new(:b).get
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Vec(11, 22)\n\"Vec\"\nfalse\n:b\n");
}

#[test]
fn a_local_reassigned_to_a_different_class_still_compiles() {
    // `local_types` is read flow-insensitively, so a retyped local must
    // widen to Poly rather than let a later class's identifier describe an
    // earlier read.
    let result = run_ruby(
        r#"
        class A
          def who = "A"
        end
        class B
          def who = "B"
        end
        x = A.new
        puts x.who
        x = B.new
        puts x.who
        y = 1
        y = "str"
        puts y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "A\nB\nstr\n");
}

#[test]
fn module_function_promotes_bareword_and_symbol_forms() {
    // A bare `module_function` promotes every following `def` to a module
    // method; `module_function :name` promotes an already-defined one.
    let result = run_ruby(
        r#"
        module M
          module_function
          def shout(s) = s.upcase
        end
        module N
          def whisper(s) = s.downcase
          module_function :whisper
        end
        puts M.shout("hi")
        puts N.whisper("HI")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "HI\nhi\n");
}

#[test]
fn module_ordering_operators_and_subclasses() {
    let result = run_ruby(
        r#"
        class Animal; end
        class Dog < Animal; end
        class Cat < Animal; end
        p(Dog < Animal)
        p(Animal < Dog)
        p(Dog < Cat)
        p(Dog <= Dog)
        p(Animal > Dog)
        p(Dog <=> Animal)
        p(Dog <=> Cat)
        p(Dog <=> 5)
        p(Integer < Numeric)
        begin
          Dog < 5
        rescue TypeError => e
          puts e.message
        end
        class Base; end
        class Kid1 < Base; end
        class Kid2 < Base; end
        class GKid < Kid1; end
        p Base.subclasses.length
        p Base.subclasses.map { |c| c.to_s }.sort
        p Kid1.subclasses.map(&:to_s)
        p Base.singleton_class?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\nnil\ntrue\ntrue\n-1\nnil\nnil\ntrue\ncompared with non class/module\n2\n[\"Kid1\", \"Kid2\"]\n[\"GKid\"]\nfalse\n"
    );
}

#[test]
fn module_include_predicate() {
    let result = run_ruby(
        r#"
        module Greetable; end
        class Person; include Greetable; end
        p Person.include?(Greetable)
        p Person.include?(Comparable)
        begin
          Person.include?(Object)
        rescue TypeError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\nwrong argument type Class (expected Module)\n"
    );
}

#[test]
fn string_lines_chomp_and_prepend_variadic() {
    let result = run_ruby(
        r#"
        p "a\nb\nc".lines
        p "a\nb\nc".lines(chomp: true)
        p "a\r\nb\r\nc\r\n".lines(chomp: true)
        p "a-b-c".lines("-")
        collected = []
        "x\ny\n".each_line(chomp: true) { |l| collected << l }
        p collected
        t = "world"
        t.prepend("hello ", "big ")
        p t
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"a\\n\", \"b\\n\", \"c\"]\n[\"a\", \"b\", \"c\"]\n[\"a\", \"b\", \"c\"]\n[\"a-\", \"b-\", \"c\"]\n[\"x\", \"y\"]\n\"hello big world\"\n"
    );
}

#[test]
fn class_allocate_skips_initialize() {
    // `Class#allocate` builds an instance WITHOUT running `initialize` (so a
    // required-arg `initialize` is bypassed); the object is still a usable
    // instance whose ivars start nil and can be assigned by hand. A builtin
    // value class allocates its empty value, and a variable-held class
    // dispatches like the constant.
    let result = run_ruby(
        r#"
        class Thing
          def initialize(x) = (@x = x)
          def x = @x
        end
        t = Thing.allocate
        p t.class.name
        p t.x
        t2 = Thing.allocate
        p t2.is_a?(Thing)
        p String.allocate
        p Array.allocate
        p Hash.allocate
        w = Thing
        p w.allocate.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"Thing\"\nnil\ntrue\n\"\"\n[]\n{}\nThing\n",);
}

#[test]
fn basic_object_subclass_is_a_blank_slate() {
    // Kernel is mixed into Object, so it sits BELOW BasicObject in the chain
    // (object.c:4550 -> class.c:1853) and a BasicObject subclass never sees
    // it -- the blank slate is pure chain position, not a special case.
    // `send`/`public_send` are Kernel's; only `__send__` is BasicObject's
    // (vm_eval.c:2960). An absent method must raise even when its result is
    // immediately used as a receiver (`a.dup.own`).
    let result = run_ruby(
        r#"
        class BO < BasicObject
          def initialize; @x = 1; end
          def greet; "hi"; end
          def own; @x; end
        end
        a = BO.new
        r1 = (a.class rescue $!.class); p r1
        p a.greet
        r2 = (a.inspect rescue "no-inspect"); p r2
        r3 = (a.respond_to?(:greet) rescue "no-respond_to"); p r3
        r4 = (a.send(:greet) rescue $!.class); p r4
        p a.__send__(:greet)
        p(a == a)
        p(a == BO.new)
        p a.equal?(a)
        r5 = (a.dup.own rescue $!.class); p r5
        p a.own
        class Normal; end
        p Normal.new.class
        p Normal.new.respond_to?(:inspect)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "NoMethodError\n\"hi\"\n\"no-inspect\"\n\"no-respond_to\"\nNoMethodError\n\"hi\"\n\
         true\nfalse\ntrue\nNoMethodError\n1\nNormal\ntrue\n",
    );
}

#[test]
fn a_bare_opened_class_can_be_reparented_by_a_later_reopen() {
    // DIVERGENCE, deliberate: MRI rejects this split, but it arises in zeo
    // from wholesale-inlined libraries where the bare opening (often just to
    // hold a nested class) and the real declaration are separated. The parent
    // must come from the reopen that declares it, or a subclass override
    // would dispatch against the wrong chain. A GENUINE conflict --
    // `class Sub < A` then `class Sub < B` -- still errors.
    let result = run_ruby(
        r#"
        module M
          class Base
            def run; hook ? "blocked" : "ok"; end
            def hook; false; end
          end
          class Sub
            class Nested
              def z; 1; end
            end
          end
          class Sub < Base
            def extra; "x"; end
          end
        end
        class Child < M::Sub
          def hook; true; end
        end
        puts Child.new.run
        puts M::Sub.new.run
        puts M::Sub.new.extra
        puts M::Sub::Nested.new.z
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "blocked\nok\nx\n1\n");

    let err = zeo::compile_to_rust(
        "class A\nend\nclass B\nend\nclass Sub < A\nend\nclass Sub < B\nend\n",
    )
    .unwrap_err();
    assert!(
        err.contains("superclass mismatch for class Sub"),
        "a genuine conflict must still error: {err}"
    );
}

/// MonitorMixin is the Ruby half of `monitor` over the native reentrant lock.
#[test]
fn monitor_mixin_layers_over_the_native_reentrant_lock() {
    let result = run_ruby(
        r##"
        require "monitor"
        class Counter
          include MonitorMixin
          def initialize
            mon_initialize
            @n = 0
          end
          def bump; synchronize { @n += 1 }; end
          def reentrant; synchronize { synchronize { mon_owned? } }; end
          attr_reader :n
        end
        c = Counter.new
        c.bump
        c.bump
        p c.n
        p c.reentrant
        p c.mon_owned?
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\ntrue\nfalse\n");
}

/// `class Point < Struct.new(:x, :y)` -- a superclass that only exists at RUN
/// time (a `Struct`/`Data` class, a `Class.new`, a constant holding either).
/// The subclass can't be one of the statically emitted Rust structs, so it is
/// minted at runtime too; `super`, `superclass`, and a namespaced name all
/// have to keep working through it. `class D ... end` reopening such a
/// constant installs onto the existing class rather than registering a fresh
/// (memberless) one, so a generated reader still resolves in the body.
#[test]
fn subclassing_and_reopening_a_runtime_class() {
    let result = run_ruby(
        r#"
        class Point < Struct.new(:x, :y)
          def dist2; x * x + y * y; end
        end
        p Point.new(3, 4).dist2
        p Point.superclass.ancestors.include?(Struct)

        Base = Class.new do
          def greet; "base"; end
        end
        class Child < Base
          def greet; "child+" + super; end
        end
        p Child.new.greet
        p Child.superclass.equal?(Base)

        D = Data.define(:v)
        class D
          def double; v * 2; end
        end
        p D.new(5).double

        module NS; end
        class NS::Item < Struct.new(:a)
          def show; "a=#{a}"; end
        end
        p NS.constants
        p NS::Item.name
        p NS::Item.new(1).show
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "25\ntrue\n\"child+base\"\ntrue\n10\n[:Item]\n\"NS::Item\"\n\"a=1\"\n"
    );
}

/// Reopening a constant that holds a runtime class normally lowers to a runtime
/// reopen, but a body the block form can't express falls back to the STATIC
/// path rather than erroring. The shape that forced this: a constant alias to a
/// builtin (`INT_ALIAS = 1.class; class INT_ALIAS; include M; end`, the
/// to_words gem's pattern) briefly stopped compiling at all -- a worse failure
/// than the wrong answer it already had.
#[test]
fn reopening_a_runtime_class_falls_back_rather_than_failing_to_compile() {
    let compiles = |src: &str| compile_project(&[("main.rb", src)], "main.rb", &[]).is_ok();
    assert!(compiles(
        "module M; end\nFoo = Class.new\nclass Foo\n  include M\nend\n"
    ));
    assert!(compiles(
        "y = 99\nFoo = Class.new\nclass Foo\n  y = 1\nend\n"
    ));
    assert!(compiles(
        "Foo = Class.new\nclass Foo\n  private\n  def h; 1; end\nend\n"
    ));
    // The runtime path is still taken when the body IS expressible -- a
    // generated Data reader has to resolve in the reopened body.
    let result = run_ruby(
        "D = Data.define(:x)\nclass D\n  def double; x * 2; end\nend\np D.new(3).double\n",
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

/// A value subclass (`class S < String`) carries its payload behind a bridge
/// that re-wraps a builtin's return value into the subclass when it hands back
/// the SAME handle -- the no-allowlist heuristic for self-returning mutators
/// (`push`/`concat`). The identity CONVERSIONS return that same handle too but
/// must DEMOTE to the base class, and CRuby draws the line precisely: `to_s`/
/// `to_str`/`to_a`/`to_h` demote, while `to_ary`/`to_hash` return self. Getting
/// it wrong was not merely a wrong class -- a subclass whose `<=>` read
/// `o.to_s <=> to_s` never reached a plain String, so the user method
/// re-dispatched until the stack overflowed.
#[test]
fn value_subclass_conversions_demote_but_mutators_rewrap() {
    let result = run_ruby(
        r#"
        class S < String; end
        class A < Array; end
        class H < Hash; end
        s = S.new("x"); a = A.new([1]); h = H.new
        puts s.to_s.class
        puts s.to_str.class
        puts a.to_a.class
        puts h.to_h.class
        puts a.to_ary.class
        puts h.to_hash.class
        puts a.dup.push(2).class
        puts s.dup.concat("y").class
        puts s.upcase.class
        puts a.map { |v| v }.class
        class Backwards < String
          def <=>(o); o.to_s <=> to_s; end
        end
        p [Backwards.new("aaa"), Backwards.new("bbb")].sort.map(&:to_s)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "String\nString\nArray\nHash\nA\nH\nA\nS\nString\nArray\n[\"bbb\", \"aaa\"]\n"
    );
}

#[test]
fn struct_inspect_labels_non_identifier_members_as_symbols() {
    // A Struct/Data member whose name isn't a plain identifier prints with a
    // leading colon in inspect (`:verbose?=`), while a plain one stays bare.
    let result = run_ruby(
        r#"
        S = Struct.new(:verbose?, :name)
        p S.new(true, "x").inspect
        D = Data.define(:ok?, :count)
        p D.new(ok?: false, count: 3).inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"#<struct S :verbose?=true, name=\\\"x\\\">\"\n\
         \"#<data D :ok?=false, count=3>\"\n"
    );
}

#[test]
fn struct_self_equality_and_keyword_init_tri_state() {
    // `s == s` must not deadlock: comparing a struct to itself used to lock the
    // same slots Mutex twice and hang. `keyword_init?` is tri-state, matching
    // CRuby -- `nil` when never specified, else the boolean passed.
    let result = run_ruby(
        r#"
        S = Struct.new(:a, :b)
        s = S.new(1, 2)
        p(s == s)
        p(s == S.new(1, 2))
        p(s == S.new(1, 9))
        p S.keyword_init?
        p Struct.new(:a, keyword_init: true).keyword_init?
        p Struct.new(:a, keyword_init: false).keyword_init?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\nnil\ntrue\nfalse\n");
}

#[test]
fn a_static_top_level_if_guard_registers_or_drops_its_definitions() {
    // The `if defined?(Const) ... module M ... end` idiom (singleton's
    // `if defined?(Ractor)` tail): a decidably-TRUE guard's branch is spliced
    // through the top-level walk (its module registers, its methods resolve),
    // a decidably-FALSE guard's branch is dropped entirely -- exactly the
    // branch real Ruby would or wouldn't execute there. Nesting folds too.
    let result = run_ruby(
        r#"
        if defined?(String)
          module Kept
            def self.tag
              "kept"
            end
          end
          if defined?(Integer)
            module KeptNested
              def self.tag
                "nested"
              end
            end
          end
        end
        if defined?(NoSuchConstantAnywhere)
          module Dropped
            def self.tag
              "dropped"
            end
          end
        end
        puts Kept.tag
        puts KeptNested.tag
        puts defined?(Dropped).inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "kept\nnested\nnil\n");
}

#[test]
fn a_user_method_named_clone_does_not_shadow_the_internal_arc_clone() {
    // A Ruby method named `clone` becomes an INHERENT `fn clone` on the
    // generated struct; every internal self/local copy must therefore avoid
    // `.clone()` method syntax (which would resolve to the Ruby method --
    // infinite recursion in `clone`'s own body, type errors elsewhere).
    // Exercises the collision through self-reference, ivar writes (the
    // frozen-check emission), and an Object-typed local re-read.
    let result = run_ruby(
        r#"
        class Uncopyable
          def initialize
            @n = 1
          end
          def clone
            raise TypeError, "can't clone #{self.class}"
          end
          def bump
            @n += 1
            self
          end
          def n
            @n
          end
        end
        u = Uncopyable.new
        u.bump
        puts u.n
        begin
          u.clone
        rescue TypeError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\ncan't clone Uncopyable\n");
}

#[test]
fn super_from_an_extended_module_method_resolves_the_singleton_chain() {
    // `extend M` puts M in the receiver's SINGLETON-class chain -- a `super`
    // inside M's method used to panic the compiler ("not found in own
    // ancestors"); it now resolves that chain statically (most recent extend
    // first), falling back to the runtime walk for builtin defaults.
    let result = run_ruby(
        r#"
        module Base
          def greet
            "base"
          end
        end
        module Loud
          def greet
            super + "!"
          end
        end
        class Host
          extend Base
          extend Loud
        end
        puts Host.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "base!\n");
}

#[test]
fn reopened_prelude_and_parent_methods_resolve_on_subclass_instances() {
    // The dispatch guard tests for the MRO-walk fallback (`lookup_mro`):
    // a reopened exception-prelude class's method visible on rescued
    // subclass instances, late parent reopens and mid-chain module
    // includes visible on deep-leaf instances. Expected output is
    // verbatim ruby 4.0.5.
    let result = run_ruby(
        r##"
        class StandardError
          def tagged; "SE-tag: #{message}"; end
        end
        begin
          raise ArgumentError, "boom"
        rescue => e
          puts e.tagged
        end
        class Base
          def hello; "base hello"; end
        end
        class Mid < Base; end
        class Leaf < Mid; end
        puts Leaf.new.hello
        class Base
          def late; "late method"; end
        end
        puts Leaf.new.late
        module Mixin
          def mixed; "mixed in"; end
        end
        class Mid
          include Mixin
        end
        puts Leaf.new.mixed
        class Exception
          def exc_tag; "exc: #{self.class}"; end
        end
        begin
          raise "r"
        rescue => e
          puts e.exc_tag
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "SE-tag: boom\n\
         base hello\n\
         late method\n\
         mixed in\n\
         exc: RuntimeError\n"
    );
}

#[test]
fn class_bodies_execute_at_their_document_position_and_rerun_per_reopen() {
    // Real Ruby runs a class/module body WHERE IT APPEARS, interleaved
    // with surrounding top-level code, re-executing each reopen's body at
    // its own site; a rescued raise inside a module body shows the
    // `<module:M>` frame and `<main>` at the `module` keyword's line.
    // (Bodies used to be hoisted wholesale to the head of `run_main`,
    // printing before earlier top-level output and reading line 0.)
    let result = run_ruby(
        r#"
        puts "top1"
        class Foo
          puts "body1"
        end
        puts "top2"
        class Foo
          puts "body2"
        end
        class Outer
          puts "outer start"
          class Inner
            puts "inner body"
          end
          puts "outer end"
        end
        module M
          begin
            raise "inmod"
          rescue => e
            puts "rescued: #{e.backtrace[0]}"
            puts "from: #{e.backtrace[1]}"
          end
        end
        puts "top3"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "top1\nbody1\ntop2\nbody2\nouter start\ninner body\nouter end\n\
         rescued: -e:19:in '<module:M>'\nfrom: -e:17:in '<main>'\ntop3\n"
    );
}

#[test]
fn sibling_extend_super_chains_resolve_in_extension_order() {
    // `extend A; extend B; extend C` builds the singleton chain most-recent
    // first (#<Class:Host> -> C -> B -> A -> ...), and `super` walks it per
    // POSITION: the flattened class-method table keeps only winners, so
    // shadowed sibling copies get their own emitted fns registered as
    // singleton super targets (the own_impls distinction, singleton side).
    // A chain also crosses from extends into a parent's `def self.x`.
    // All three outputs verbatim from ruby 4.0.5.
    let result = run_ruby(
        r#"
        module A
          def who; "A -> top"; end
        end
        module B
          def who; "B -> " + super; end
        end
        module C
          def who; "C -> " + super; end
        end
        class Host
          extend A
          extend B
          extend C
          def self.who; "Host -> " + super; end
        end
        puts Host.who
        class Host2
          extend A
          extend B
        end
        puts Host2.who
        class Parent
          def self.who; "Parent -> top"; end
        end
        class Kid < Parent
          extend B
          def self.who; "Kid -> " + super; end
        end
        puts Kid.who
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Host -> C -> B -> A -> top\nB -> A -> top\nKid -> B -> Parent -> top\n"
    );
}
