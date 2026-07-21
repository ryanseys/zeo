# Issue #119: an attr_reader-defined accessor on a polymorphic
# receiver was lost — the cls_id-keyed dispatcher in
# `compile_poly_method_call` only walked `@cls_meth_names` and never
# checked `@cls_attr_readers`, so the if-cascade emitted an empty
# body and the result temp stayed at its `0` default. The same shape
# with an explicit `def v; @v; end` worked fine.
#
# Fix: poly_dispatch_return_type now also unions ivar types from
# each class's attr_readers, and the cls_id-keyed cascade emits a
# `_t = ((sp_<C> *)recv.v.p)->iv_<name>` branch for classes that
# only declare the accessor as an attr_reader.

class C
  def initialize(v); @v = v; end
  attr_reader :v
end

class D
  def initialize(v); @v = v; end
  attr_reader :v
end

# Same return type per class (string) — result temp stays
# `const char *`, no boxing.
def make(flag)
  flag ? C.new("c-val") : D.new("d-val")
end

puts make(true).v
puts make(false).v

# Mixing an explicit `def v` on one class with attr_reader on
# others. The dispatcher must cover both shapes.
class F
  def initialize(v); @v = v; end
  def v; @v; end
end

def mix(flag)
  flag ? C.new("from-attr") : F.new("from-def")
end

puts mix(true).v
puts mix(false).v
