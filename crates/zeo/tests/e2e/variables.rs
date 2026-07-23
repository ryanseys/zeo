use crate::support::{compile_project, run_ruby};

#[test]
fn eval_can_write_an_ivar_on_self() {
    let result = run_ruby(
        r#"
        class Foo
          def initialize
            eval("@x = 42")
          end
          def x
            @x
          end
        end
        puts Foo.new.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn an_escaping_block_can_mutate_an_ivar_via_self_capture() {
    // Exercises the self:Rc<Self> migration + self-capture path: the block
    // is written inside `Box#run` (so `self`/`@sum` are in scope there) and
    // passed to a call on an EXPLICIT other receiver (`c`, a Collector
    // constructed directly as a LOCAL, not taken as a method parameter --
    // method params are always statically `Poly` in this spike, a separate
    // pre-existing gap; a `New`-assigned local's class IS statically known,
    // so this routes around that while still exercising real self-capture.
    // Implicit-self calls to a user method are ALSO a separate, unrelated,
    // still-unsupported gap, avoided here the same way).
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        class Box
          def initialize
            @sum = 0
          end
          def total
            @sum
          end
          def run
            c = Collector.new
            c.each_num(1, 2, 3) { |n| @sum += n }
          end
        end
        b = Box.new
        b.run
        puts b.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn a_class_variable_is_shared_with_a_subclass_that_never_declares_its_own() {
    let result = run_ruby(
        r#"
        class Base
          @@count = 0
          def bump
            @@count += 1
          end
          def count
            @@count
          end
        end
        class Sub < Base
          def bump_twice
            @@count += 1
            @@count += 1
          end
        end
        b = Base.new
        s = Sub.new
        b.bump
        s.bump_twice
        puts b.count
        puts s.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n3\n");
}

#[test]
fn a_subclass_reassigning_a_cvar_mutates_the_shared_inherited_storage() {
    // Verified against real Ruby first: a subclass's own `@@x = ...`
    // does NOT shadow -- it finds and mutates the SAME storage inherited
    // from the superclass (real Ruby's actual, if slightly surprising,
    // class-variable semantics). This is also the exact scenario zeo's
    // own C implementation gets wrong (a subclass writing a superclass-only
    // cvar allocates fresh, separate storage there -- an outright compile
    // failure in zeo's case; see the plan's Part 6).
    let result = run_ruby(
        r#"
        class Base
          @@x = 1
          def base_x
            @@x
          end
        end
        class Sub < Base
          @@x = 99
          def sub_x
            @@x
          end
        end
        puts Base.new.base_x
        puts Sub.new.sub_x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "99\n99\n");
}

#[test]
fn an_unrelated_class_own_cvar_is_independent_storage() {
    let result = run_ruby(
        r#"
        class Base
          @@count = 0
          def count
            @@count
          end
        end
        class Other
          @@count = 100
          def count
            @@count
          end
        end
        puts Base.new.count
        puts Other.new.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n100\n");
}

// --- Deeper include/extend/prepend/inherited-ivar coverage,
// oracle-verified against real `ruby` first (added after the initial batch
// above, per the user's request for more comprehensive coverage of these
// specifically).

#[test]
fn a_non_overridden_inherited_initialize_flattens_ivars_onto_the_subclass_struct() {
    let result = run_ruby(
        r#"
        class Parent
          def initialize(x)
            @x = x
          end
        end
        class Child < Parent
          def show
            @x * 10
          end
        end
        puts Child.new(4).show
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "40\n");
}

#[test]
fn an_included_module_method_mutates_an_ivar_on_the_includer() {
    // Real self/ivar capture for a materialized module method -- proves
    // the module body was re-typechecked against the INCLUDER's own
    // concrete struct, not some shared/aliased representation.
    let result = run_ruby(
        r#"
        module Counter
          def bump
            @count += 1
          end
        end
        class Widget
          include Counter
          def initialize
            @count = 0
          end
          def count
            @count
          end
        end
        w = Widget.new
        w.bump
        w.bump
        w.bump
        puts w.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn a_module_class_variable_is_shared_across_every_including_class() {
    // Verified against real Ruby first: `@@total` declared inside the
    // MODULE's own body is genuinely owned by the module itself -- every
    // class that includes it (even two entirely unrelated classes) shares
    // the SAME storage, not one copy per includer. Exercises
    // `codegen::expr::cvar_owner_id` consulting `Ctx.defining_class`
    // (the module a materialized method's body actually came from), not
    // `Ctx.current_class` (whichever class it's materialized onto).
    let result = run_ruby(
        r#"
        module Shared
          @@total = 0
          def add(n)
            @@total += n
          end
          def total
            @@total
          end
        end
        class Left
          include Shared
        end
        class Right
          include Shared
        end
        l = Left.new
        r = Right.new
        l.add(5)
        r.add(7)
        puts l.total
        puts r.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "12\n12\n");
}

#[test]
fn ivars_from_a_three_level_plain_inheritance_chain_are_all_present_on_the_leaf() {
    // A REAL bug found while adding this test: ivar-flattening originally
    // only scanned the MRO-winning materialized methods, missing any ivar
    // only ever touched through a `super`-reachable (but shadowed, not
    // winning) ancestor body -- `B#initialize`/`C#initialize` both call
    // `super` and their OWN bodies don't textually mention `@a`, only
    // `A#initialize`'s spliced-in body does. Fixed by collecting ivars from
    // EVERY ancestor's own methods, not just the ones that end up
    // materialized as the winning definition.
    let result = run_ruby(
        r#"
        class A
          def initialize
            @a = 1
          end
        end
        class B < A
          def initialize
            super
            @b = 2
          end
        end
        class C < B
          def initialize
            super
            @c = 3
          end
          def total
            @a + @b + @c
          end
        end
        puts C.new.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn optional_param_default_expr_referencing_an_ivar_is_scanned() {
    // Before this fix, `analyze::collect_ivars`/`locals::track_extra`/
    // `hoisting`'s per-method scans never walked into a `Params::optional`/
    // `keywords` default expression -- an ivar referenced ONLY there (never
    // in the method's own body) would be missing from the generated
    // struct's fields entirely, a "no such field" codegen error. Also
    // exercises the keyword-optional case (`y:`) in the same call.
    let result = run_ruby(
        r#"
        class Greeter
          def initialize
            @default_name = "World"
          end
          def greet(x = @default_name, y: @default_name)
            "hi #{x} and #{y}"
          end
        end

        puts Greeter.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi World and World\n");
}

#[test]
fn implicit_self_call_with_args_can_mutate_an_ivar() {
    let result = run_ruby(
        r#"
        class Counter
          def initialize
            @count = 0
          end

          def bump(n)
            add(n)
            @count
          end

          def add(n)
            @count = @count + n
          end
        end

        c = Counter.new
        puts c.bump(5)
        puts c.bump(2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n7\n");
}

#[test]
fn class_level_ivars_are_per_class_and_not_inherited() {
    // The defining property of class-level `@x`, and the whole reason it
    // can't share `@@x`'s storage: a subclass gets its OWN slot, starting
    // empty, even though it inherits the method that reads it. Contrast the
    // `@@cv` line, which IS shared. Oracle-verified (ruby 4.0.5).
    let result = run_ruby(
        r#"
        class Base
          @reg = "base-ivar"
          @@cv = "base-cvar"
          def self.reg; @reg; end
          def self.reg=(v); @reg = v; end
          def self.cv; @@cv; end
          def self.unset; @never_written; end
        end
        class Sub < Base; end
        p Base.reg
        p Sub.reg
        p Sub.cv
        p Base.unset
        Sub.reg = "sub-only"
        p [Base.reg, Sub.reg]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"base-ivar\"\nnil\n\"base-cvar\"\nnil\n[\"base-ivar\", \"sub-only\"]\n"
    );
}

#[test]
fn a_class_ivar_and_an_instance_ivar_of_the_same_name_are_distinct_storage() {
    // `@x` in a class body/class method and `@x` in an instance method name
    // two completely different slots -- the class object's own, and the
    // instance's. Also exercises the same-name collision across Ruby's two
    // method namespaces (`def self.x` + `def x`), which share one generated
    // container and so need `ident::class_method_ident`'s mangling.
    let result = run_ruby(
        r#"
        class C
          @x = "class-level"
          def initialize; @x = "instance-level"; end
          def self.x; @x; end
          def x; @x; end
        end
        p [C.x, C.new.x]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"class-level\", \"instance-level\"]\n");
}

#[test]
fn class_shovel_self_attr_accessor_backs_onto_class_level_ivars() {
    // `class << self; attr_accessor :x; end` is THE idiomatic way to declare
    // class-level state, and it works by generating `def self.x; @x; end` --
    // so it only works once class-level `@x` has real storage.
    let result = run_ruby(
        r#"
        module Reg
          class << self
            attr_accessor :handler
            def helper; "helped"; end
          end
        end
        Reg.handler = "H"
        p Reg.handler
        p Reg.helper

        class Cfg
          class << self
            attr_reader :mode
            attr_writer :mode
          end
          @mode = "default"
        end
        p Cfg.mode
        Cfg.mode = "custom"
        p Cfg.mode
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"H\"\n\"helped\"\n\"default\"\n\"custom\"\n"
    );
}

#[test]
fn a_class_ivar_is_reachable_from_a_block_inside_a_class_method() {
    // The block captures `self` as a plain `RubyValue::Class`, so the ivar
    // resolves through `ivar_get_dyn`/`ivar_set_dyn`'s Class arm rather than
    // the static class-id path -- the two must agree on the same storage.
    let result = run_ruby(
        r#"
        class Blk
          @vals = []
          def self.collect
            [1, 2, 3].each { |i| @vals << i * 10 }
            @vals
          end
        end
        p Blk.collect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[10, 20, 30]\n");
}

#[test]
fn class_objects_answer_the_instance_variable_reflection_family() {
    let result = run_ruby(
        r#"
        class K
          @a = 1
        end
        p K.instance_variable_get(:@a)
        p K.instance_variable_get("@a")
        p K.instance_variable_get(:@nope)
        p K.instance_variables
        p K.instance_variable_set(:@b, 2)
        p K.instance_variables
        p K.instance_variable_defined?(:@a)
        p K.instance_variable_defined?(:@zz)
        begin
          K.instance_variable_get(:a)
        rescue NameError => e
          puts "NameError: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n1\nnil\n[:@a]\n2\n[:@a, :@b]\ntrue\nfalse\n\
         NameError: 'a' is not allowed as an instance variable name\n"
    );
}

// Globals, namespaced constants, compound-assignment and
// multi-assignment completeness, call-site splats. Every test below is
// oracle-verified against real `ruby` first, per this project's established
// convention.

#[test]
fn global_variables_read_write_and_default_to_nil() {
    let result = run_ruby(
        r#"
        $x = 10
        puts $x
        puts $never_set
        $x += 5
        puts $x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n\n15\n");
}

#[test]
fn top_level_constant_read_and_write() {
    let result = run_ruby("MAX = 100\nputs MAX");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n");
}

#[test]
fn constant_declared_in_a_superclass_resolves_from_a_subclass_method() {
    let result = run_ruby(
        r#"
        class Base
          X = 1
        end
        class Sub < Base
          def get
            X
          end
        end
        puts Sub.new.get
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn namespaced_constant_read_via_double_colon() {
    let result = run_ruby(
        r#"
        class Foo
          BAR = 42
        end
        puts Foo::BAR
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn a_bare_constant_still_falls_back_to_the_top_level_from_inside_a_class() {
    let result = run_ruby(
        r#"
        MAX_ITEMS = 5
        class Config
          LIMIT = 3
          def show
            puts LIMIT
            puts MAX_ITEMS
          end
        end
        Config.new.show
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n5\n");
}

#[test]
fn multi_assign_to_ivar_and_global_targets() {
    let result = run_ruby(
        r#"
        class Thing
          attr_reader :x
          def set_both(g)
            @x, $g2 = 10, g
          end
        end
        t = Thing.new
        t.set_both(99)
        puts t.x
        puts $g2
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n99\n");
}

#[test]
fn alias_of_a_method_touching_an_ivar_works_through_either_name() {
    let result = run_ruby(
        r#"
        class Counter
          def initialize
            @count = 0
          end
          def increment
            @count += 1
          end
          alias inc increment
          def value
            @count
          end
        end
        c = Counter.new
        c.inc
        c.inc
        c.increment
        puts c.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn class_shift_self_methods_can_read_and_write_class_variables() {
    let result = run_ruby(
        r#"
        class Counter
          class << self
            def reset
              @@total = 0
            end
            def bump
              @@total += 1
            end
            def total
              @@total
            end
          end
        end
        Counter.reset
        Counter.bump
        Counter.bump
        puts Counter.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn class_shift_a_constant_object_defines_its_singleton() {
    // #97 F3: `class << CONST` on a constant-bound object installs per-object
    // singleton methods on it (previously a clean rejection).
    let result = run_ruby(
        r#"
        class Foo
          ANOTHER = Object.new
          class << ANOTHER
            def hi
              "hi"
            end
          end
        end
        puts Foo::ANOTHER.hi
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\n");
}

#[test]
fn class_shift_self_constants_are_visible_to_the_singletons_class_methods() {
    // #96 harness-surfaced: the dominant `class << self` stdlib idiom (e.g.
    // URI's `class << self; RESERVED = ...; def escape; ...RESERVED...; end`)
    // defines constants alongside the class methods that reference them. The
    // constant is spliced onto the enclosing class, whose class methods resolve
    // it lexically (oracle-verified).
    let result = run_ruby(
        r##"
        class Config
          class << self
            PREFIX = "cfg:"
            LIMIT = 3
            def key(n); "#{PREFIX}#{n}"; end
            def capped(n); n > LIMIT ? LIMIT : n; end
          end
        end
        puts Config.key("host")
        puts Config.capped(9)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "cfg:host\n3\n");
}

#[test]
fn reading_the_same_ivar_or_captured_local_twice_in_one_expression_does_not_deadlock() {
    // CRITICAL bug, found via the operator-method test above (`@x * @x`
    // inside `Vector#<=>`): `codegen::expr`'s `IvarRead` and
    // `codegen::hoisting`'s captured-`LocalRead` both emitted a bare,
    // UNNAMED `#expr.lock().clone()` -- Rust's temporary-lifetime rule
    // keeps an unnamed `.lock()` guard alive until the end of the
    // ENCLOSING STATEMENT (confirmed via a minimal, standalone
    // `parking_lot::Mutex` repro), so referencing the SAME ivar or
    // captured local TWICE in one expression/statement (a very common
    // shape: squaring, self-comparison, `total - total`, not just the
    // already-audited read-modify-WRITE case Part 9 covered) silently
    // deadlocked the whole generated program forever -- no panic, no
    // error, just a permanent hang. Fixed by binding the guard to an
    // explicit named local INSIDE its own block (confirmed empirically
    // that a bare `{ }` wrapper alone does NOT change the drop timing --
    // only a named `let` binding does).
    let result = run_ruby(
        r#"
        class Squarer
          def initialize(n)
            @n = n
          end
          def square
            @n * @n
          end
        end
        puts Squarer.new(7).square

        class Once
          def run
            yield
          end
        end
        total = 5
        block_result = 0
        Once.new.run { block_result = total * total }
        puts block_result
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "49\n25\n");
}

#[test]
fn or_assign_on_a_never_before_defined_constant_quietly_defines_it() {
    // A real, previously-undetected bug: `CONST ||= v` on a constant that
    // was NEVER assigned raised `NameError` instead of quietly defining
    // it -- confirmed via real `ruby` that this is a genuine, narrow
    // special case (ONLY `||=` gets this leniency; `+=`/`&&=` on the same
    // undefined constant still raise, see the next test). Fixed via a new
    // `HirNode::ConstReadOrNil`, used only by `||=`'s own desugar.
    let result = run_ruby(
        r#"
        MAX ||= 100
        puts MAX
        MAX ||= 200
        puts MAX
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n100\n");
}

#[test]
fn compound_assignment_on_a_plain_non_class_constant_dispatches_correctly() {
    // A real, previously-undetected bug, found alongside the `||=` fix
    // above: `codegen::call::emit_call` treated ANY `HirNode::ClassRef`
    // receiver as a `ClassName.foo(...)` class-method call, UNCONDITIONALLY
    // -- but `ClassRef` is also how an ordinary bare constant read lowers
    // (see its own docs). So `MAX += 1` (desugared to `MAX.+(1)`, where
    // `MAX` is a plain `Integer` constant, not a class) panicked with a
    // confusing "unknown class/module `MAX`" instead of reading/writing
    // its actual value -- EVERY compound-assignment operator except `||=`
    // on any non-class constant was completely broken. Fixed by only
    // taking the class-method-call branch when the name is ACTUALLY a
    // registered class/module.
    let result = run_ruby(
        r#"
        MAX = 100
        MAX += 1
        puts MAX
        MAX -= 50
        puts MAX
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "101\n51\n");
}

#[test]
fn module_metaprogramming_attr_class_eval_exec_and_invented_ivars() {
    // Batch 2: attr (== attr_reader), class_eval/module_exec block forms whose
    // added methods are inherited by subclasses and includers, and an
    // invented ivar (assigned by a class_eval/instance_exec body, never
    // declared on the struct) surviving in the per-object overflow map.
    let result = run_ruby(
        r#"
        class C001
          attr :x
          def initialize; @x = 5; end
        end
        p C001.new.x

        class Box
          def initialize(v); @v = v; end
        end
        class BoxPlus < Box; end
        Box.class_eval do
          def doubled; @v * 2; end
          define_method(:tripled) { @v * 3 }
          def labelled; @label = "n=#{@v}"; @label; end
        end
        b = Box.new(21)
        p [b.doubled, b.tripled, b.labelled]
        p BoxPlus.new(5).doubled

        module M; end
        M.module_exec { def mm; "mm"; end }
        class D; include M; end
        p D.new.mm

        o = Object.new
        o.instance_exec { @invented = 99 }
        p o.instance_variable_get(:@invented)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n[42, 63, \"n=21\"]\n10\n\"mm\"\n99\n");
}

#[test]
fn object_protocol_remove_ivar_singleton_class_method_and_extend() {
    // Batch 1 object/Kernel protocol: remove_instance_variable (value + NameError),
    // singleton_class (on an object and a builtin value), singleton_method, and
    // extend across every module kind -- a user compile-time module and a builtin
    // module (Comparable) -- reaching the object's singleton table.
    let result = run_ruby(
        r#"
        class Box
          def initialize; @v = 1; @s = "hi"; end
          def drop; remove_instance_variable(:@v); end
        end
        b = Box.new
        p b.drop
        p(begin; b.remove_instance_variable(:@nope); rescue NameError => e; e.message; end)
        p "x".singleton_class.class
        p Object.new.singleton_class.superclass
        o = Object.new
        def o.greet; "hey"; end
        p o.singleton_method(:greet).call
        sc = Object.new
        sc.singleton_class.define_method(:doubled) { 21 * 2 }
        p sc.doubled
        module Greet; def hi(n); "hi #{n}"; end; end
        u = Object.new
        u.extend(Greet)
        p u.hi("ada")
        n = Object.new
        n.extend(Comparable)
        p n.respond_to?(:clamp)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n\"instance variable @nope not defined\"\nClass\nObject\n\"hey\"\n42\n\"hi ada\"\ntrue\n"
    );
}

#[test]
fn default_object_inspect_and_to_s_carry_address_and_ivars() {
    // Default (no override) `to_s` is `#<Class:0xADDR>`; `inspect` adds the
    // ivars as `@name=<inspected>` in declaration order. Addresses are
    // normalized in-program (as the corpus tests do) so the expectation is
    // stable. A frozen-mutation FrozenError message carries the same inspect.
    let result = run_ruby(
        r#"
        def norm(s) = s.gsub(/0x[0-9a-f]+/, "0xADDR")
        class Widget
          def initialize(n); @name = n; @size = 3; end
        end
        w = Widget.new("gadget")
        puts norm(w.to_s)
        puts norm(w.inspect)
        class Empty; end
        puts norm(Empty.new.inspect)
        puts norm(Object.new.inspect)
        class Frozen
          attr_accessor :v
          def initialize; @v = 1; freeze; end
        end
        begin
          Frozen.new.v = 2
        rescue FrozenError => e
          puts norm(e.message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<Widget:0xADDR>\n\
         #<Widget:0xADDR @name=\"gadget\", @size=3>\n\
         #<Empty:0xADDR>\n\
         #<Object:0xADDR>\n\
         can't modify frozen Frozen: #<Frozen:0xADDR @v=1>\n"
    );
}

#[test]
fn dup_and_clone_on_user_objects_copy_ivars_shallowly() {
    let result = run_ruby(
        r#"
        class Point
          attr_accessor :x, :y

          def initialize(x, y)
            @x = x
            @y = y
          end
        end

        p1 = Point.new(1, 2)
        p2 = p1.dup
        p2.x = 99
        puts p1.x
        puts p2.x
        puts p2.y
        p1.freeze
        puts p1.clone.frozen?
        puts p1.dup.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n99\n2\ntrue\nfalse\n");
}

// -- Nested classes/modules + constant paths (namespacing).
// Every positive expectation oracle-verified against real ruby 4.0.5.

#[test]
fn nested_classes_define_dispatch_and_resolve_lexical_constants() {
    let result = run_ruby(
        r#"
        module Store
          DEFAULT = 10

          class Item
            def price
              DEFAULT
            end
          end

          class Errors
            class NotFound
              def msg
                "not found"
              end
            end
          end
        end

        puts Store::Item.new.price
        puts Store::Errors::NotFound.new.msg
        puts Store::DEFAULT
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\nnot found\n10\n");
}

#[test]
fn bare_constant_resolution_walks_the_lexical_chain_innermost_first() {
    let result = run_ruby(
        r#"
        X = "top"
        module Outer
          X = "outer"
          module Inner
            X = "inner"
            def self.probe
              X
            end
          end
          def self.probe
            X
          end
        end
        puts Outer::Inner.probe
        puts Outer.probe
        puts X
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "inner\nouter\ntop\n");
}

#[test]
fn constants_resolve_through_the_superclass_chain() {
    let result = run_ruby(
        r#"
        class Base2
          LIMIT = 5
        end

        class Sub2 < Base2
          def l
            LIMIT
          end
        end

        puts Sub2.new.l
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n");
}

#[test]
fn qualified_constant_writes_target_the_named_namespace() {
    let result = run_ruby(
        r#"
        module Cfg
        end

        Cfg::LIMIT = 99
        puts Cfg::LIMIT
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "99\n");
}

#[test]
fn a_class_stored_in_a_variable_constructs_and_dispatches() {
    let result = run_ruby(
        r#"
        class Widget
          def initialize(n)
            @n = n
          end

          def n
            @n
          end
        end

        x = Widget
        puts x.new(5).n
        puts x.name
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\nWidget\n");
}

/// Class-level state on a builtin: `@@cvar` + `CONST` in the body (ordinary
/// ownership machinery, owner = the builtin's ClassId), `def self.x` via the
/// generated `__bm_Array` container, and `Array::LIMIT` readable externally.
#[test]
fn builtin_reopen_cvars_consts_and_class_methods() {
    let result = run_ruby(
        r#"
        class Array
          @@made = 0
          LIMIT = 3

          def self.tally_up
            @@made = @@made + 1
            @@made
          end

          def under_limit?
            length < LIMIT
          end
        end

        puts Array.tally_up
        puts Array.tally_up
        puts [1, 2].under_limit?
        puts [1, 2, 3, 4].under_limit?
        puts Array::LIMIT
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\ntrue\nfalse\n3\n");
}

/// Math module functions + Math::DomainError + the Float constants, and
/// the Kernel conversion functions with CRuby's exact failure shapes.
#[test]
fn math_constants_and_kernel_conversions() {
    let result = run_ruby(
        r##"
        puts Math::PI
        puts Math::E
        puts Math.sqrt(16)
        puts Math.sqrt(2)
        puts Math.cbrt(27)
        puts Math.log2(8)
        puts Math.log(Math::E)
        puts Math.log(8, 2)
        puts Math.hypot(3, 4)
        puts Math.atan2(1, 1)
        begin
          Math.sqrt(-1)
        rescue Math::DomainError => e
          puts "Math::DomainError: #{e.message}"
        end
        puts Float::INFINITY
        puts Float::EPSILON
        puts Float::MAX
        puts Float::DIG
        puts Float::RADIX
        puts Integer("42")
        puts Integer("ff", 16)
        puts Integer("0x1A")
        puts Integer(" -4_2 ")
        puts Integer(3.9)
        puts Float("1.5e3")
        puts Float(2)
        puts String(42)
        puts Array(nil).inspect
        puts Array(1..3).inspect
        puts Array(5).inspect
        puts Hash(nil).inspect
        p Rational(3, 4)
        p Complex(1, 2)
        begin
          Integer("nope")
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        begin
          Integer(nil)
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        begin
          Float("x")
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3.141592653589793\n2.718281828459045\n4.0\n1.4142135623730951\n3.0\n3.0\n1.0\n3.0\n\
         5.0\n0.7853981633974483\nMath::DomainError: Numerical argument is out of domain - sqrt\n\
         Infinity\n2.220446049250313e-16\n1.7976931348623157e+308\n15\n2\n\
         42\n255\n26\n-42\n3\n1500.0\n2.0\n42\n[]\n[1, 2, 3]\n[5]\n{}\n(3/4)\n(1+2i)\n\
         ArgumentError: invalid value for Integer(): \"nope\"\n\
         TypeError: can't convert nil into Integer\n\
         ArgumentError: invalid value for Float(): \"x\"\n"
    );
}

// ---------------------------------------------------------------------------
// Struct: compile-time class synthesis. Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// `Point = Struct.new(:x, :y)` mints a native struct class at runtime (Batch
/// E) and the constant write names it: accessors, positional init (nil-filled),
/// members/to_a/to_h/==/[]/[]=/each_pair/inspect, and Enumerable through the
/// real ancestor chain. Plus keyword_init and the block-with-methods form. The
/// constant form now takes the exact same runtime path as the anonymous one --
/// no compile-time synthesis.
#[test]
fn const_struct_matches_the_oracle() {
    let result = run_ruby(
        r#"
        Point = Struct.new(:x, :y)
        pt = Point.new(1, 2)
        p pt
        puts pt.x
        pt.y = 9
        p pt.to_a
        p pt.to_h
        p pt.members
        p pt == Point.new(1, 9)
        p pt == Point.new(1, 2)
        p pt[0]
        p pt[:y]
        p pt["x"]
        p pt[-1]
        pt[1] = 20
        p pt.y
        p pt.length
        p pt.map { |v| v }
        p pt.select { |v| v.is_a?(Integer) }
        acc = []
        pt.each_pair { |k, v| acc << [k, v] }
        p acc
        p Point.ancestors.include?(Struct)
        p Point.ancestors.include?(Enumerable)
        Label = Struct.new(:text, keyword_init: true)
        l = Label.new(text: "hi")
        p l
        Pair = Struct.new(:a, :b) do
          def total
            a + b
          end
        end
        p Pair.new(3, 4).total
        p Point.new(5).y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<struct Point x=1, y=2>\n1\n[1, 9]\n{x: 1, y: 9}\n[:x, :y]\ntrue\nfalse\n\
         1\n9\n1\n9\n20\n2\n[1, 20]\n[1, 20]\n[[:x, 1], [:y, 20]]\ntrue\ntrue\n\
         #<struct Label text=\"hi\">\n7\nnil\n"
    );
}

/// The Batch-E flip retired compile-time Struct synthesis, so the constant form
/// is the SAME runtime mint as the anonymous form -- and the runtime
/// `Struct.new("Name", :a)` accepts the legacy string-name argument instead of
/// rejecting it at compile time. zeo names the class `Name`; CRuby's legacy
/// behaviour namespaces it `Struct::Name` -- a known divergence shared with the
/// anonymous path, not worth reintroducing a compile-time special case for.
#[test]
fn struct_new_string_name_at_const_mints_at_runtime() {
    assert!(zeo::compile_to_rust("P = Struct.new(\"Name\", :a)\n").is_ok());
}

#[test]
fn top_level_ivars_live_on_the_main_object() {
    // Was rejected as "the `main` object has no ivar storage". It has some
    // now (`dispatch::Object`'s name-keyed map), so a top-level `@x` -- read
    // or written, at the top level or from a top-level `def`, which is a
    // private method of Object whose self IS main -- is ordinary state.
    // A never-assigned one reads nil rather than raising.
    let result = run_ruby(
        r#"
        @x = 1
        p @x
        p @never
        def read_it; @x; end
        p read_it
        def bump; @count = (@count || 0) + 1; end
        bump
        bump
        p @count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\nnil\n1\n2\n");
}

#[test]
fn a_top_level_def_touching_an_ivar_does_not_taint_builtins() {
    // `mro::materialize_methods` collects ivars from every ancestor, but a
    // BUILTIN never materializes Object's methods -- so the ivar loop has to
    // skip Object for builtins exactly as the method loop does. It didn't,
    // which made this program fail to compile with a rejection blaming
    // `Integer` for a `@count` that Integer has nothing to do with.
    let result = run_ruby(
        r#"
        def bump; @count = (@count || 0) + 1; end
        bump
        p @count
        p 1 + 2
        p "s".length
        p [1, 2].map { |i| i * 2 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n3\n1\n[2, 4]\n");
}

#[test]
fn fallible_class_body_constant_and_method_default_args() {
    // `CONST = <fallible expr>` in a class body and `def m(a = <fallible>)`
    // both previously emitted `?` outside a Result context.
    let result = run_ruby(
        r#"
        def source
          41
        end
        class Config
          LIMIT = [1, 2, 3].sum
        end
        def bump(a = source + 1)
          a
        end
        puts Config::LIMIT
        puts bump
        puts bump(5)
        "#,
    );
    assert_eq!(result.stdout, "6\n42\n5\n");
}

/// The civil constructors disagree about their 7th argument on purpose:
/// `Time.utc`'s is MICROSECONDS, `Time.new`'s is a UTC OFFSET in seconds.
/// Both range-check it. Also: `inspect` renders an offset's seconds where
/// `to_s` doesn't.
#[test]
fn time_civil_constructors_and_their_seventh_argument() {
    let result = run_ruby(
        r#"
        u = Time.utc(2007, 11, 1, 15, 25, 0, 123456)
        p u.usec
        p u.nsec
        puts u.inspect
        p u.utc?

        o = Time.new(2000, 1, 1, 0, 0, 0, 3600)
        p o.utc_offset
        p o.utc?
        puts o.to_s

        # An offset that is not a whole minute: to_s truncates, inspect does not.
        s = Time.new(2000, 1, 1, 0, 0, 0, 123)
        puts s.to_s
        puts s.inspect

        begin
          Time.utc(2000, 1, 1, 0, 0, 0, 1000000)
        rescue ArgumentError => e
          puts "usec: #{e.message}"
        end
        begin
          Time.new(2000, 1, 1, 0, 0, 0, 86400)
        rescue ArgumentError => e
          puts "offset: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "123456\n123456000\n2007-11-01 15:25:00.123456 UTC\ntrue\n3600\nfalse\n2000-01-01 00:00:00 +0100\n2000-01-01 00:00:00 +0002\n2000-01-01 00:00:00 +000203\nusec: subsecx out of range\noffset: utc_offset out of range\n"
    );
}

#[test]
fn class_variables_at_module_and_top_level_scope() {
    // `@@x` written in a module body, in a `def self.` body, and bare at
    // the top level (whose storage lives on Object) all read back.
    let result = run_ruby(
        r#"
        module Conf
          @@secret = ""
          def self.secret; @@secret; end
          def self.secret=(v); @@secret = v; end
        end
        puts Conf.secret.length
        Conf.secret = "hi"
        puts Conf.secret
        @@plain = 42
        puts @@plain
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\nhi\n42\n");
}

#[test]
fn top_level_class_variable_is_stored_on_object() {
    // A bare `@@x` written outside any class/module body resolves its storage
    // to Object, so a later top-level read sees the same value. NOTE a
    // divergence: Ruby 4.0.5 itself now RAISES `RuntimeError: class variable
    // access from toplevel` for both the write and the read here (it was a
    // warning in older rubies). Zeo keeps the older permissive behavior to
    // match the committed `test/module_cvars.rb` snapshot the conformance
    // suite scores against; this test pins that intentional choice.
    let result = run_ruby(
        r#"
        @@plain = 42
        puts @@plain
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn module_const_get_and_const_defined_reflect_the_registry() {
    let result = run_ruby(
        r#"
        module M
          X = 7
          module N
          end
        end
        puts M.const_get(:X)
        puts M.const_get("X")
        p M.const_defined?(:X)
        p M.const_defined?(:Nope)
        p M.const_defined?(:N)
        p M.const_get(:N).is_a?(Module)
        begin; M.const_get(:Missing); rescue NameError => e; puts e.message; end
        begin; M.const_defined?("bad"); rescue NameError => e; puts e.message; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "7\n7\ntrue\nfalse\ntrue\ntrue\n\
         uninitialized constant M::Missing\nwrong constant name bad\n"
    );
}

#[test]
fn class_method_defined_and_class_variable_reflection() {
    let result = run_ruby(
        r#"
        class Base
          @@shared = 1
          def inherited_m; end
        end
        class Sub < Base
          def own_m; end
        end
        p Sub.method_defined?(:own_m)
        p Sub.method_defined?(:inherited_m)
        p Sub.method_defined?(:frozen?)
        p Sub.method_defined?(:nope)
        p Base.class_variable_defined?(:@@shared)
        p Base.class_variable_get(:@@shared)
        Base.class_variable_set(:@@shared, 42)
        p Base.class_variable_get(:@@shared)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\nfalse\ntrue\n1\n42\n");
}

#[test]
fn instance_variable_reflection_on_objects() {
    // get/set/list over a concrete receiver, a poly receiver, missing names,
    // an empty object, and a malformed-name NameError.
    let result = run_ruby(
        r#"
        class Box
          def initialize(v); @v = v; @tag = "b"; end
        end
        b = Box.new(7)
        p b.instance_variables
        p b.instance_variable_get(:@v)
        b.instance_variable_set(:@v, 70)
        p b.instance_variable_get("@v")
        p b.instance_variable_get(:@missing)
        class Bare; end
        p Bare.new.instance_variables
        def peek(o) = o.instance_variable_get(:@v)
        p peek(Box.new(99))
        begin
          b.instance_variable_get(:v)
        rescue NameError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:@v, :@tag]\n7\n70\nnil\n[]\n99\n\
         'v' is not allowed as an instance variable name\n"
    );
}

#[test]
fn set_construction_membership_operators_and_enumerable() {
    let result = run_ruby(
        r#"
        s = Set[3, 1, 2, 1]
        p s.size
        p s.include?(2)
        p (Set[1, 2] | Set[2, 3]).to_a.sort
        p Set[1, 2].subset?(Set[1, 2, 3])
        p s.map { |x| x * 2 }.sort
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\ntrue\n[1, 2, 3]\ntrue\n[2, 4, 6]\n");
}

#[test]
fn eval_resolves_builtin_class_and_module_constants() {
    let result = run_ruby(
        r#"
        puts eval("Integer".dup)
        puts eval("Math::PI".dup).round(2)
        puts eval("Math.sqrt(81)".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Integer\n3.14\n9.0\n");
}

#[test]
fn eval_resolves_user_constants_and_classes() {
    let result = run_ruby(
        r#"
        FOO = 42
        class Widget; end
        puts eval("FOO".dup)
        puts eval("Widget".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\nWidget\n");
}

#[test]
fn eval_sees_globals_and_main_object_ivars() {
    let result = run_ruby(
        r#"
        $g = "global"
        @iv = 41
        puts eval("$g".dup)
        puts eval("@iv + 1".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "global\n42\n");
}

#[test]
fn instance_eval_string_reads_the_receivers_ivars() {
    let result = run_ruby(
        r#"
        class Account
          def initialize(n)
            @balance = n
          end
        end
        acct = Account.new(100)
        src = "@balance + 5"
        puts acct.instance_eval(src)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "105\n");
}

#[test]
fn instance_eval_string_mutates_an_ivar() {
    let result = run_ruby(
        r#"
        class Counter
          def initialize
            @n = 0
          end
        end
        c = Counter.new
        c.instance_eval("@n = @n + 3")
        puts c.instance_eval("@n")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

// ---------------------------------------------------------------------------
// Builtin method additions (conformance drawdown): Hash[], try_convert,
// Complex.polar, Module#include?, Regexp.last_match, Thread#join/#value, and
// the Random PRNG. Ruby-visible surface; the PRNG cases assert only
// deterministic guarantees (a seeded xorshift64* diverges from CRuby's MT).
// ---------------------------------------------------------------------------

#[test]
fn hash_class_bracket_constructor() {
    let result = run_ruby(
        r#"
        p Hash[]
        p Hash[1, 2, 3, 4]
        p Hash[[[:a, 1], [:b, 2]]]
        p Hash[{ x: 1 }]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{}\n{1 => 2, 3 => 4}\n{a: 1, b: 2}\n{x: 1}\n"
    );
}

#[test]
fn complex_polar_constructor() {
    let result = run_ruby("p Complex.polar(2, 0)");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "(2+0.0i)\n");
}

#[test]
fn data_constructs_positionally_or_by_keyword() {
    let result = run_ruby(
        r#"
        Point = Data.define(:x, :y)
        a = Point.new(1, 2)
        b = Point.new(x: 1, y: 2)
        p [a.x, a.y]
        p(a == b)
        p a.frozen?
        # The `rescue` modifier needs its own parens inside a call's arguments
        # (a bare `p(x rescue y)` is a SyntaxError in ruby 4.0.5 too).
        p((Point.new(1) rescue $!.class))
        p((Point.new(x: 1, y: 2, z: 3) rescue $!.class))
        p((a.with(z: 9) rescue $!.class))
        p a.with(y: 5).to_h
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\ntrue\ntrue\nArgumentError\nArgumentError\nArgumentError\n{x: 1, y: 5}\n"
    );
}

#[test]
fn a_defined_guard_over_a_missing_constant_folds_its_dead_branch_away() {
    // The dead arm references `Absent::Thing` and calls a method that doesn't
    // exist on `Shim` -- both must be eliminated, not emitted, or the program
    // fails to compile. CRuby's reachability agrees the branch never runs.
    let result = run_ruby(
        r#"
        class Shim; end
        v = defined?(Absent::Thing) ? Absent::Thing : "fallback"
        p v
        def guard
          if defined?(NoSuchFeature) && NoSuchFeature.on?
            Shim.new.method_that_does_not_exist
            "on"
          else
            "off"
          end
        end
        p guard
        p defined?(Absent::Thing)
        p defined?(String)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"fallback\"\n\"off\"\nnil\n\"constant\"\n");
}

#[test]
fn module_constants_included_modules_and_class_variables() {
    // Module#constants (own first, ancestors' next, Object's excluded),
    // #included_modules (modules in the MRO), and #class_variables (own +
    // ancestors).
    let result = run_ruby(
        r#"
        module Walks; end
        class Animal; end
        class Dog < Animal; include Walks; end
        p Dog.included_modules
        module Mod; MC = 9; end
        class A; X = 1; end
        class B < A; Y = 2; include Mod; end
        p A.constants
        p B.constants.sort
        p B.constants(false)
        class C; @@x = 5; end
        C.class_variable_set(:@@y, 9)
        p C.class_variables.sort
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[Walks, Kernel]\n[:X]\n[:MC, :X, :Y]\n[:Y]\n[:@@x, :@@y]\n",
    );
}

#[test]
fn top_level_scoped_constant_names_the_builtin_class() {
    // `::Integer` (and other `::Name` top-level anchors) resolve to the builtin
    // class in every position: is_a?/kind_of?/instance_of? arguments, `===`
    // receivers, and as a first-class Class value.
    let result = run_ruby(
        r#"
        p 5.is_a?(::Integer)
        p "x".is_a?(::String)
        p 3.14.kind_of?(::Numeric)
        p 5.instance_of?(::Integer)
        p(::Integer === 7)
        p(::String === "a")
        p ::Integer
        p ::Array.new(2, 0)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\nInteger\n[0, 0]\n",
    );
}

#[test]
fn singleton_method_reads_ivars_and_yields() {
    let result = run_ruby(
        r##"
        obj = Object.new
        obj.instance_variable_set(:@base, 100)
        def obj.add
          @base + yield
        end
        p obj.add { 5 }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "105\n");
}

/// Ivar codegen resolves a RUNTIME `self` ahead of the LEXICAL class context
/// it inherited. A `def` nested in a `class_eval` block sits inside
/// `Gadget`'s body (so `class_self` is set) but runs under a dynamic self, and
/// its `@v` must reach the INSTANCE, not the class's own ivar table. Read and
/// write must agree on this: when only the read was fixed, `@v += 1` read the
/// instance and wrote the class table, so the increment vanished.
///
/// The `Counter`/`Mixed` halves pin the other direction -- a real class-method
/// `self` still reaches the class's own table, and a class-level `@cls` stays
/// distinct from an instance's same-named ivar.
#[test]
fn ivar_in_a_class_eval_nested_def_targets_the_instance_not_the_class() {
    let result = run_ruby(
        r#"
        class Gadget
          def initialize(v); @v = v; end
          class_eval do
            def doubled; @v * 2; end
            def bump!; @v += 1; self; end
          end
        end
        g = Gadget.new(10)
        p g.doubled
        p g.bump!.doubled

        class Counter
          def self.tick; @n = (@n || 0) + 1; end
          def self.n; @n; end
        end
        Counter.tick; Counter.tick
        p Counter.n

        class Mixed
          @cls = "class-level"
          def self.cls; @cls; end
          def initialize; @cls = "instance-level"; end
          def inst; @cls; end
        end
        p Mixed.cls
        p Mixed.new.inst
        p Mixed.cls
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "20\n22\n2\n\"class-level\"\n\"instance-level\"\n\"class-level\"\n"
    );
}

/// Singleton methods on an ordinary object. Three things that were broken
/// together: `super` inside a `class << obj` body (it panicked the compiler with
/// "`super` outside a method", since a per-object singleton has no compile-time
/// class to splice an ancestor chain against); a block passed to a runtime
/// singleton method on a class/module (`M.wrap { }` -- dispatch dropped it and
/// `yield` raised LocalJumpError); and `define_singleton_method` on a constant
/// that holds an OBJECT rather than naming a class (it was desugared into a
/// class reopen, minting a phantom class, so `B.class` answered `Class`).
#[test]
fn singleton_methods_on_objects_constants_and_modules() {
    let result = run_ruby(
        r#"
        class Widget
          def initialize(n); @n = n; end
          def label; "w#{@n}"; end
        end
        W = Widget.new(1)
        class << W
          def label; "custom-#{super}"; end
        end
        p W.label
        p W.class

        module M; end
        def M.wrap; "[" + yield + "]"; end
        puts M.wrap { "hi" }

        class Box
          def v; 1; end
        end
        B = Box.new
        B.define_singleton_method(:doubled) { v * 2 }
        p B.class
        p B.doubled

        class Named; end
        Named.define_singleton_method(:greet) { "hello" }
        p Named.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"custom-w1\"\nWidget\n[hi]\nBox\n2\n\"hello\"\n"
    );
}

/// `class X < <expression>` builds the class at runtime, so its body runs as a
/// block -- and a construct only the static class path can emit has no runtime
/// spelling. Reaching codegen with one used to abort the compiler with an
/// "unexpected top-level-only node in expression position" panic; a local write
/// would silently assign the ENCLOSING scope's variable instead of opening its
/// own. Both are rejected by name.
///
/// A REOPEN (`Foo = Class.new; class Foo ... end`) has a static fallback for
/// exactly these bodies, so it takes that instead of rejecting -- see
/// `reopening_a_runtime_class_falls_back_rather_than_failing_to_compile`.
#[test]
fn a_local_write_in_a_runtime_class_body_is_a_clean_rejection_not_a_panic() {
    // A local assignment in a runtime-built class body would see the enclosing
    // scope's locals rather than its own, so it stays a clean compile error
    // (`include`/`alias`/a nested class are now REWRITTEN to runtime self-sends
    // -- see `transform_runtime_class_body` -- rather than rejected).
    let src = "y = 99\nclass Foo < Struct.new(:a)\n  y = 1\nend\n";
    let err = compile_project(&[("main.rb", src)], "main.rb", &[])
        .expect_err("expected a compile-time rejection");
    assert!(
        err.contains("local variable assignment in the body of `class Foo`"),
        "got: {err}"
    );
}

#[test]
fn multiple_assignment_used_as_an_expression_yields_the_raw_rhs() {
    // `(a, b = rhs)` in value position yields the RHS verbatim, as CRuby does:
    // a call returning an array yields that array (not a fresh copy), a bare
    // scalar yields the scalar (not `[scalar]`), an implicit list yields the
    // list. The destructuring still happens; the expression's own value is the
    // untouched RHS. Regression: the sub-expression form used to yield `nil`,
    // so an outer subscript/comparison hit `[]`/`<` on nil.
    let result = run_ruby(
        r#"
        def two_ints; [10, 20]; end
        result = (x, y = two_ints)
        p x
        p y
        p result
        first = (p1, p2, p3 = [7, 8, 9])[0]
        p first
        flag = (a, b = [3, 4])[0] == 3
        p flag
        p((c, d = 5))
        p((e, f = 1, 2))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n20\n[10, 20]\n7\ntrue\n5\n[1, 2]\n");
}
