# Issue #413. A class method `def []=(key, value)` that dispatches
# on `key` and assigns into ivars of mixed types (Integer #@id,
# String @name) emitted with the `[]=` body present in C but the
# call site `f[:id] = 42` silently dropped — `compile_bracket_assign`
# fell through every typed-array / hash arm and had no arm for a
# user-class receiver, so the IndexAssignNode produced no output.
#
# Fix: add an `is_obj_type(rt)` arm at the end of
# `compile_bracket_assign`. It looks the user class up via
# `find_class_idx`, walks the inheritance chain via
# `find_method_owner` for `"[]="`, and emits the typed dispatch
# (`sp_<owner>__aset(recv, key, value)`) with arg boxing matched
# to the method's declared param types via the existing
# `compile_typed_call_args` helper. So a `value` param widened
# to `sp_RbVal` (because the case branches assign to ivars of
# different concrete types) takes a boxed rhs; a homogeneously-
# typed param takes it raw.
#
# The accompanying `lv_value` parameter monomorphisation
# concern from the issue body (the function signature was said
# to commit `lv_value` to `mrb_int`) was already addressed by
# unrelated inference passes by the time this fix landed — the
# signature now correctly reads `sp_RbVal lv_value` for the
# multi-type-ivar shape. The remaining gap was purely the
# missing dispatch at the call site.
#
# Coverage:
#   - Mixed-type ivars (`@id` int, `@name` string) — the
#     canonical Rails-fixture shape from the issue's repro.
#   - Homogeneous-type ivars to verify the dispatch still
#     works through the simpler typed-param path.

class Foo
  attr_reader :id, :name
  def initialize
    @id = 0
    @name = ""
  end
  def []=(key, value)
    case key
    when :id
      @id = value
    when :name
      @name = value
    end
  end
end

f = Foo.new
f[:id] = 42
f[:name] = "alice"
puts f.id.to_s              # 42
puts f.name                 # alice

class Bag
  attr_reader :a, :b
  def initialize
    @a = 0
    @b = 0
  end
  def []=(key, value)
    case key
    when :a
      @a = value
    when :b
      @b = value
    end
  end
end

bg = Bag.new
bg[:a] = 7
bg[:b] = 11
puts (bg.a + bg.b).to_s     # 18

