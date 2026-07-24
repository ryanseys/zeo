# Issue #379: when a method returns `obj_<C> | nil` and Spinel
# collapses the union to `mrb_int` upstream, the local catching
# the call ends up declared `mrb_int`. Use sites that need the
# concrete class then differ in their handling: property accesses
# (`local.id`) get a runtime cast inserted by
# `compile_int_class_fallback_expr`, while call args that pass
# the local to a typed-pointer parameter (`f(local)` where the
# param is `sp_<C> *`) were previously emitted raw — gcc errored
# `incompatible integer to pointer conversion`.
#
# Fix: in compile_call_args_with_defaults' positional-arg branch,
# when the param's declared ptype is `obj_<C>` and the arg's
# inferred type is `int`, insert the same `(sp_<C> *)` cast that
# the property-access path already emits. Same machinery now
# wired into both directions.

class Holder
  attr_reader :name
  def initialize(name)
    @name = name
  end
end

class Wrapper
  attr_reader :tag
  def initialize(tag)
    @tag = tag
  end
end

# Force the helper's return to widen across two concrete obj
# returns so the resulting union collapses (`mrb_int`):
def maybe_holder(flag)
  if flag == 0
    Holder.new("h0")
  else
    Wrapper.new("w0")
  end
end

def consume_holder(h)
  h.name
end

# Push instances into a heterogeneous array so neither class
# stays value-typed.
all = [Holder.new("init"), Wrapper.new("init")]

h = maybe_holder(0)
all << h

# Without the fix this call site would emit
# `consume_holder(lv_h)` against a `consume_holder(sp_Holder *)`
# signature, with lv_h still typed mrb_int — type-mismatched C.
puts consume_holder(h)
