# Bundled tests:
#   - fiber_ivar_persists_across_yield
#   - fiber_yield_across_method_call
#   - file_write_poly_arg
#   - forward_call_class_method_inherited_init
#   - forward_call_class_method_inherited_init_int_array

# === fiber_ivar_persists_across_yield ===
class T_fiber_ivar_persists_across_yield_Counter
  def initialize
    @n = 0
    @fiber = Fiber.new do
      @n += 10
      Fiber.yield
      @n += 100
      Fiber.yield
      @n += 1000
    end
  end

  def step
    @fiber.resume
    @n
  end
end

c = T_fiber_ivar_persists_across_yield_Counter.new
puts c.step
puts c.step
puts c.step

# === fiber_yield_across_method_call ===
class T_fiber_yield_across_method_call_Stepper
  def initialize
    @count = 0
    @fiber = Fiber.new { main_loop }
  end

  def step
    @fiber.resume
    @count
  end

  def main_loop
    while @count < 5
      tick
      tick
      Fiber.yield
    end
  end

  def tick
    @count += 1
  end
end

s = T_fiber_yield_across_method_call_Stepper.new
puts s.step
puts s.step
puts s.step

# === file_write_poly_arg ===
# Issue #643: `File.write(path, value)` where `value`'s static
# type is poly (cross-class union widened the return slot of the
# producer to sp_RbVal) used to emit `sp_file_write(lv_path,
# lv_value)` with a struct value passed where const char * was
# expected — C compile failure with "incompatible type for
# argument 2".
#
# Fix: when either arg's static type is poly, extract `.v.s`
# before the call.

class T_file_write_poly_arg_Worker
  def run(x)
    "result-#{x}"
  end
end

class T_file_write_poly_arg_OtherWorker
  def run(x)
    "alt-#{x}"
  end
end

# Cross-class union via array — both classes' `run` returns String,
# but the union of obj_Worker | obj_OtherWorker pushed through
# `.each` widens the dispatch return to sp_RbVal in some shapes.
workers = [T_file_write_poly_arg_Worker.new, T_file_write_poly_arg_OtherWorker.new]
results = workers.map { |w| w.run(42) }

# results is poly_array; result of [0] dispatch is poly. File.write
# call site must unbox before the sp_file_write boundary.
# Use cwd-relative path so Windows MinGW (no `/tmp`) passes —
# memory: feedback_windows_tmp_path.
path = "spinel_i643_test.txt"
File.write(path, results[0])
puts File.read(path)
File.delete(path)

# === forward_call_class_method_inherited_init ===
# Forward-ref `Class.new(arg)` whose Class is a subclass with its
# own `def initialize(_arg); super; end`. Pre-fix codegen widened
# the child's #initialize ptype from the call site but never
# propagated the type through the bare `super` to the parent's
# #initialize, leaving the parent's `owner` param at the default
# `mrb_int`. The parent's body `@owner = owner` then stored the
# int payload; subsequent `@owner.<method>` dispatches on a child
# instance landed on the wrong class via the int→class fallback.

class T_forward_call_class_method_inherited_init_Outer
  def initialize
    @child = T_forward_call_class_method_inherited_init_Sub.new(self)
  end
  def report
    @child.read_owner_name
  end
  def name; "outer"; end
end

class T_forward_call_class_method_inherited_init_Base
  def initialize(owner)
    @owner = owner
  end
  def read_owner_name
    @owner.name
  end
end

class T_forward_call_class_method_inherited_init_Sub < T_forward_call_class_method_inherited_init_Base
  def initialize(_owner)
    super
  end
end

o = T_forward_call_class_method_inherited_init_Outer.new
puts o.report

# === forward_call_class_method_inherited_init_int_array ===
# Inherited-init forward-ref with an IntArray param flowing through
# the super call. The parent's `arr` param must end up
# `sp_IntArray *` (not the default `mrb_int`) so the body's
# `@arr = arr` stores the right pointer type and a parent-defined
# accessor reads the typed value.

class T_forward_call_class_method_inherited_init_int_array_Driver
  def initialize
    @child = T_forward_call_class_method_inherited_init_int_array_Subscriber.new([10, 20, 30, 40])
  end
  def report
    @child.length
  end
end

class T_forward_call_class_method_inherited_init_int_array_Listener
  def initialize(arr)
    @arr = arr
  end
  def length
    @arr.length
  end
end

class T_forward_call_class_method_inherited_init_int_array_Subscriber < T_forward_call_class_method_inherited_init_int_array_Listener
  def initialize(_arr)
    super
  end
end

d = T_forward_call_class_method_inherited_init_int_array_Driver.new
puts d.report

