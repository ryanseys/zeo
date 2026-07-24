# Issue #419. `obj.class` was unresolved on an instance -- emitted
# `cannot resolve call to 'class' on obj_<C> (emitting 0)` and
# downstream methods cascaded through int -> 0. Sibling to #404
# Phase 1 (which closed the literal-reference direction `c = Foo`):
# this is the reflection direction, fetching the class FROM an
# instance.
#
# Fix:
#   - Codegen (compile_object_method_expr's is_obj_type arm): when
#     mname is "class", emit `((sp_Class){<cls_idx>LL})`.
#   - Codegen (compile_object_method_expr's recv_type=="class"
#     arm): detect chained `<obj>.class.<cmeth>` via AST shape
#     (recv is a `class` CallNode whose own recv has a known
#     obj_<C> type) and dispatch directly to `sp_<C>_cls_<m>`,
#     bypassing the runtime sp_Class value.
#   - Inference: infer_method_name_type returns "class" for `obj.class`
#     and the cmeth's return type for `<obj>.class.<cmeth>`.
#   - DCE (collect_cls_calls): when the recv is a `class` CallNode,
#     mark cmeth live on every class that defines one. Over-
#     approximates because LocalVariableReadNode types aren't
#     resolvable at DCE time (they're intentionally not cached
#     by walk_and_cache), but DCE prefers a kept-but-unused
#     method over a stripped live one.
#   - String interpolation (`"#{self.class}"`): treat sp_Class
#     value via sp_class_to_s, not (long long)-cast which is
#     wrong for a struct.
#
# Coverage: chained .class.<cmeth>, plain `obj.class.to_s`
# (uses the recv_type=="class" -> to_s arm from #404), and the
# string-interpolation form `"#{self.class}"` that optcarrot's
# default inspect uses.

class Foo
  def self.greet
    "hello-from-foo"
  end
end

f = Foo.new
puts f.class.greet              # hello-from-foo
puts f.class.to_s               # Foo

class Bar
  attr_accessor :v
  def initialize
    @v = 0
  end
  def describe
    "#<#{ self.class }>"
  end
end

b = Bar.new
puts b.describe                 # #<Bar>

# Subclass: dispatch should pick up the cmeth defined on the
# inherited class. Spinel's cmeth lookup walks parents via
# cls_cmethod_owner, mirroring CRuby's dispatch.
class Base
  def self.kind
    "base-kind"
  end
end

class Child < Base
end

c = Child.new
puts c.class.kind               # base-kind
