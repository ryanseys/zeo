# Bundled tests:
#   - scan_new_calls_class_scope
#   - self_class_subclass_dispatch

# === scan_new_calls_class_scope ===
# `scan_new_calls` walks the AST looking for `Foo.new(args)` and
# propagates each arg's inferred type into Foo's initialize param-
# type table. The arg inference uses `infer_type`, which for an
# `InstanceVariableReadNode` consults `@current_class_idx` —
# previously the scan never set it.
#
# Result: `Bar.new(@x, ...)` inside `Foo#initialize` resolved
# `@x` against an empty scope (returned the int default), so
# Bar.initialize's first param wedged at mrb_int. The fix pins
# `@current_class_idx` while recursing into ClassNode bodies.

class T_scan_new_calls_class_scope_A
  attr_reader :n
  def initialize(n)
    @n = n
  end
end

class T_scan_new_calls_class_scope_B
  attr_reader :a
  def initialize(a_arg)
    @a = a_arg
  end
end

class T_scan_new_calls_class_scope_C
  attr_reader :b
  def initialize
    @inner = T_scan_new_calls_class_scope_A.new(7)
    @b = T_scan_new_calls_class_scope_B.new(@inner)
  end
end

c = T_scan_new_calls_class_scope_C.new
puts c.b.a.n   # 7

# === self_class_subclass_dispatch ===
# Issue #422. `self.class.<cmeth>` inside a parent-defined
# instance method must dispatch to the subclass override at
# runtime, not to the parent's cmeth. Pre-fix the codegen
# resolved cmeth statically against the method body's defining
# class, so a T_self_class_subclass_dispatch_Child instance routed through a T_self_class_subclass_dispatch_Base-defined
# `describe` always landed on `T_self_class_subclass_dispatch_Base.label`.
#
# Fix shape:
#   - lib structs: every non-value-type sp_<C> gets `mrb_int
#     cls_id` as its first field (layout-roots emit it; subclasses
#     inherit via parent-fields-first ordering, so a cast preserves
#     the offset).
#   - constructors: `sp_<C>_new` writes `self->cls_id = <C's idx>`
#     so the runtime carries the concrete class tag.
#   - chained dispatch: `<recv>.class.<cmeth>()` lowers to a
#     `switch (<recv>->cls_id)` when descendants of recv's static
#     class override the cmeth.
#
# Coverage:
#   - Plain T_self_class_subclass_dispatch_Base/T_self_class_subclass_dispatch_Child override.
#   - Multi-level (T_self_class_subclass_dispatch_GrandChild overrides label).
#   - Subclass that doesn't override -- inherits via the default
#     (T_self_class_subclass_dispatch_Sibling has no .label, falls through to T_self_class_subclass_dispatch_Base).
#   - Direct T_self_class_subclass_dispatch_Base instance still routes to T_self_class_subclass_dispatch_Base.

class T_self_class_subclass_dispatch_Base
  def self.label
    "BASE"
  end

  def describe
    self.class.label
  end
end

class T_self_class_subclass_dispatch_Child < T_self_class_subclass_dispatch_Base
  def self.label
    "CHILD"
  end
end

class T_self_class_subclass_dispatch_GrandChild < T_self_class_subclass_dispatch_Child
  def self.label
    "GRANDCHILD"
  end
end

class T_self_class_subclass_dispatch_Sibling < T_self_class_subclass_dispatch_Base
end

puts T_self_class_subclass_dispatch_Base.new.describe
puts T_self_class_subclass_dispatch_Child.new.describe
puts T_self_class_subclass_dispatch_GrandChild.new.describe
puts T_self_class_subclass_dispatch_Sibling.new.describe

