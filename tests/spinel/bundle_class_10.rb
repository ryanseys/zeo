# Bundled tests:
#   - forward_call_class_method_inherited_init_obj
#   - forward_call_class_method_int_array
#   - forward_call_class_method_nested
#   - forward_call_class_method_obj
#   - forward_call_param_type_int_array

# === forward_call_class_method_inherited_init_obj ===
# Inherited-init forward-ref where the param type is itself an
# obj_X user class. Verifies the type flows through both the
# `Class.new(@h)` constructor branch and the bare `super` of the
# child's initialize.

class T_forward_call_class_method_inherited_init_obj_Holder
  def initialize(name)
    @name = name
  end
  def label
    "h:#{@name}"
  end
end

class T_forward_call_class_method_inherited_init_obj_Outer
  def initialize
    @h = T_forward_call_class_method_inherited_init_obj_Holder.new("alpha")
    @child = T_forward_call_class_method_inherited_init_obj_Sub.new(@h)
  end
  def report
    @child.held_label
  end
end

class T_forward_call_class_method_inherited_init_obj_Base
  def initialize(holder)
    @holder = holder
  end
  def held_label
    @holder.label
  end
end

class T_forward_call_class_method_inherited_init_obj_Sub < T_forward_call_class_method_inherited_init_obj_Base
  def initialize(_holder)
    super
  end
end

o = T_forward_call_class_method_inherited_init_obj_Outer.new
puts o.report

# === forward_call_class_method_int_array ===
# `Class.new(int_array_arg)` where the callee class is defined later
# than the call site. Pre-fix codegen left T_forward_call_class_method_int_array_Target's `arr` param at
# the default `mrb_int`; the int→class fallback then emitted the
# call with a Wint-conversion error. Source order kept caller-first.

class T_forward_call_class_method_int_array_Caller
  def make_target
    @t = T_forward_call_class_method_int_array_Target.new([10, 20, 30])
  end
  def length
    @t.show
  end
end

class T_forward_call_class_method_int_array_Target
  def initialize(arr)
    @arr = arr
  end
  def show
    @arr.length
  end
end

c = T_forward_call_class_method_int_array_Caller.new
c.make_target
puts c.length

# === forward_call_class_method_nested ===
# Nested forward-ref: `T_forward_call_class_method_nested_Outer.new(T_forward_call_class_method_nested_Inner.new)` where both T_forward_call_class_method_nested_Outer and
# T_forward_call_class_method_nested_Inner are defined later than the construction site. The T_forward_call_class_method_nested_Inner.new
# nested arg type must propagate to T_forward_call_class_method_nested_Outer.initialize's param so the
# outer Class.new ptype widens correctly.

class T_forward_call_class_method_nested_Driver
  def boot
    @outer = T_forward_call_class_method_nested_Outer.new(T_forward_call_class_method_nested_Inner.new(42))
  end
  def reach
    @outer.payload.value
  end
end

class T_forward_call_class_method_nested_Inner
  def initialize(v)
    @v = v
  end
  def value
    @v
  end
end

class T_forward_call_class_method_nested_Outer
  def initialize(inner)
    @inner = inner
  end
  def payload
    @inner
  end
end

d = T_forward_call_class_method_nested_Driver.new
d.boot
puts d.reach

# === forward_call_class_method_obj ===
# `Class.new(obj_X)` shape: caller passes a user-class instance to a
# forward-defined Class.new. Mirrors the optcarrot
# `PPU.new(@conf, @cpu, @video.palette)` site where the callee's
# obj-typed param flowed in from a yet-to-be-defined sibling class.

class T_forward_call_class_method_obj_Holder
  def initialize(name)
    @name = name
  end
  def label
    "h:#{@name}"
  end
end

class T_forward_call_class_method_obj_Caller
  def make
    @h = T_forward_call_class_method_obj_Holder.new("alpha")
    @t = T_forward_call_class_method_obj_Target.new(@h)
  end
  def show
    @t.held_label
  end
end

class T_forward_call_class_method_obj_Target
  def initialize(h)
    @holder = h
  end
  def held_label
    @holder.label
  end
end

c = T_forward_call_class_method_obj_Caller.new
c.make
puts c.show

# === forward_call_param_type_int_array ===
# File-order caller-first / callee-second: a T_forward_call_param_type_int_array_Caller class invokes a
# method on a yet-untyped ivar (because the callee class is defined
# later in the source). Pre-fix codegen left the callee's params at
# the default `mrb_int` so the IntArray + bool args produced
# "incompatible-pointer" / "Wint-conversion" errors when the
# int→class fallback emitted the call. With the forward-ref widening
# in place the callee picks up `sp_IntArray *` + `mrb_bool` from
# this single call site.

class T_forward_call_param_type_int_array_Caller
  def initialize(target)
    @target = target
    @arr = [1, 2, 3]
    @flag = false
  end
  def go
    @target.set_payload(@arr, @flag)
  end
end

class T_forward_call_param_type_int_array_Target
  def set_payload(arr, flag)
    @arr = arr
    @flag = flag
  end
  def info
    "len=#{@arr.length} flag=#{@flag}"
  end
end

t = T_forward_call_param_type_int_array_Target.new
c = T_forward_call_param_type_int_array_Caller.new(t)
c.go
puts t.info

