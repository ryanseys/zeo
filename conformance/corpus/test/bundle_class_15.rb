# Bundled tests:
#   - inherited_method_param_widen
#   - init_locals
#   - initialize_param_poly_call_sites
#   - initialize_param_through_call
#   - initialize_void_wrapper_gc_save

# === inherited_method_param_widen ===
# Issue #286: a parent class's instance method param needs to widen
# from subclass call sites too, not just from same-class call sites.
# Previously scan_new_calls's bare-call branch only checked
# `find_method_idx` (top-level methods); a `bar(arg)` inside a
# subclass that resolves through inheritance to the parent's `bar`
# would leave the parent's param frozen at its initial type and
# fail to type-check at the call site.
#
# Fix: when the bare call is inside a class method body
# (@current_class_idx >= 0), walk up the inheritance chain via
# find_method_owner to locate the ancestor that defines the
# method, then widen *that* class's @cls_meth_ptypes from the
# call-site arg types — same shape the recv >= 0 branch already
# uses for explicit-receiver calls.

class T_inherited_method_param_widen_T
  def assert_eq(expected, actual)
    if expected == actual
      puts "ok"
    else
      puts "ng"
    end
  end
end

class T_inherited_method_param_widen_Article
  def initialize
    @id = 0
  end
  def write_str(s)
    @id = s   # widens @id to poly (#247)
  end
  def id
    @id
  end
end

class T_inherited_method_param_widen_T2 < T_inherited_method_param_widen_T
  def test_it
    a = T_inherited_method_param_widen_Article.new
    a.write_str("hi")
    # Bare call to inherited assert_eq, passing a poly value (a.id).
    # Without the fix, `T_inherited_method_param_widen_T#assert_eq`'s `actual` param stays at int
    # and the call site emits a type-mismatched store.
    assert_eq("hi", a.id)
  end
end

T_inherited_method_param_widen_T2.new.test_it

# === init_locals ===
# Regression test for issue #17: locals inside `initialize` were not
# declared in the generated `_new` constructor (and `_initialize`
# helper for value-type classes), so cc rejected the body with
# `'lv_x' undeclared`.

class T_init_locals_Test
  attr_reader :a
  def initialize
    x = 1
    @a = x
  end
end

class T_init_locals_Sum
  attr_reader :total
  def initialize(n)
    s = 0
    i = 0
    while i < n
      s = s + i
      i = i + 1
    end
    @total = s
  end
end

class T_init_locals_Compound
  attr_reader :result
  def initialize(x, y)
    a = x * 2
    b = y + 1
    @result = a + b
  end
end

t = T_init_locals_Test.new
puts t.a              # 1

s = T_init_locals_Sum.new(5)
puts s.total          # 10

c = T_init_locals_Compound.new(3, 4)
puts c.result         # 11

# === initialize_param_poly_call_sites ===
# Regression test: when an `initialize` parameter is widened to "poly"
# by conflicting Foo.new(...) call sites, the param type must stay poly
# end-to-end. The extended merge logic explicitly returns existing_pt
# when it is "poly" so that body inference (which can return a narrower
# concrete type via super-call propagation or via the @ivar's type when
# the ivar was seeded by a literal write elsewhere) does not silently
# narrow the param back.

class T_initialize_param_poly_call_sites_Box
  def initialize(v)
    @v = v
  end

  def show
    puts @v
  end
end

T_initialize_param_poly_call_sites_Box.new("hello").show
T_initialize_param_poly_call_sites_Box.new(42).show

# === initialize_param_through_call ===
# Regression test: an initialize parameter that is never written to an
# ivar must still pick up its type from `Foo.new(...)` call sites.
#
# Before the fix, body inference (which returns "int" when no `@x = x`
# write is found) unconditionally overwrote the call-site-inferred type,
# silently miscompiling code where the parameter was a string, array, or
# any other concrete type.

class T_initialize_param_through_call_Greeter
  def initialize(name)
    puts name
  end
end

T_initialize_param_through_call_Greeter.new("hello")

# === initialize_void_wrapper_gc_save ===
# Issue #314 follow-up: the synthesized void
# `sp_<C>_initialize` super-chain wrapper inherits @in_gc_scope
# from the prior `sp_<C>_new` constructor emit. When that left
# @in_gc_scope at 1, declare_method_locals' `@in_gc_scope == 0`
# guard skipped emitting `SP_GC_SAVE()` at the top of the void
# wrapper. But the body's bare `return` (e.g. `return if x.nil?`)
# still emitted `SP_GC_RESTORE()` — which references `_gc_saved`
# that the missing SP_GC_SAVE never declared.
#
# Fix: reset @in_gc_scope to 0 when entering the void wrapper so
# declare_method_locals decides cleanly.
#
# The void wrapper is only emitted when a subclass calls `super`,
# so this reproducer needs the inheritance shape. The body uses
# a String pointer local (which forces declare_method_locals'
# has_gc_locals branch to want SP_GC_SAVE) and an early bare
# `return` (which forces compile_return_stmt to emit
# SP_GC_RESTORE).

class T_initialize_void_wrapper_gc_save_Base
  attr_reader :extra
  def initialize(skip)
    note = "base"
    if skip
      return        # bare `return` — emits SP_GC_RESTORE()
    end
    @extra = note
  end
end

class T_initialize_void_wrapper_gc_save_Child < T_initialize_void_wrapper_gc_save_Base
  def initialize
    super(false)    # calls sp_Base_initialize (the void wrapper)
  end
end

# Push into a [T_initialize_void_wrapper_gc_save_Child] array so neither class is value-typed —
# the void wrapper is only emitted for heap classes.
all = [T_initialize_void_wrapper_gc_save_Child.new, T_initialize_void_wrapper_gc_save_Child.new]
puts all.length          # 2

