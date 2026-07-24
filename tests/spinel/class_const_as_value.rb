# Issue #404 Phase 1. A class constant referenced as a value (not
# as the receiver of `.new` / a class method call) used to emit a
# bare C identifier:
#
#   c = Row     ->  lv_c = Row;             /* Row undeclared */
#   puts c.to_s ->  warning: cannot resolve call to 'to_s' on class_Row
#
# Phase 1 wires the minimum surface needed for class-as-value to
# round-trip through a local:
#   - sp_Class struct (mrb_int cls_id) in sp_runtime.h.
#   - infer_type for ConstantReadNode / ConstantPathNode resolving
#     to a registered class returns "class".
#   - c_type "class" -> "sp_Class"; c_default_val "class" ->
#     ((sp_Class){-1LL}).
#   - compile_expr's ConstantReadNode arm emits a sp_Class compound
#     literal carrying the class index instead of the bare name.
#   - compile_object_method_expr's recv_type == "class" branch
#     lowers `.to_s` to the new sp_class_to_s helper, which
#     bound-checks against the per-program sp_class_names[] table
#     emitted by emit_class_runtime.
#
# Out of scope for Phase 1 (deferred to later work):
#   - `.name` / `.inspect` / `.==` / `.!=` on Class values.
#   - Module distinction (Module vs Class kind).
#   - Dispatch through a Class-typed local (`c.new(...)`).
#   - is_a?(Klass-typed-variable), case/when on Class values.
#   - Precomputed ancestors / .superclass / `.<` / `.<=`.
#
# Phase 2 will pick those up; the issue stays open until then.

class Row
  attr_accessor :x
end

class Article < Row
end

# Local assignment from a class constant.
c = Row
puts c.to_s                   # Row

# Same shape with a subclass.
d = Article
puts d.to_s                   # Article

# Pass a Class value through a typed-pointer round trip via an
# instance method that reads it off an ivar -- exercises the
# "store sp_Class on the heap and read it back" path that the
# bare `Foo` reference originally broke at the C-emit boundary.
class Holder
  attr_accessor :klass
end

h = Holder.new
h.klass = Row
puts h.klass.to_s             # Row
h.klass = Article
puts h.klass.to_s             # Article
