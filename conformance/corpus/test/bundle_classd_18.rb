# Bundled tests:
#   - poly_dispatch_arity_padding
#   - poly_dispatch_arity_truncate

# === poly_dispatch_arity_padding ===
# Poly dispatch over user classes whose `m` definitions have
# different arities — T_poly_dispatch_arity_padding_Switch#toggle takes 0 args; T_poly_dispatch_arity_padding_Knob#toggle takes
# one positional default. Without per-arm padding, the C compiler
# rejects the dispatch table with "too few arguments to function
# 'sp_Knob_toggle'" (the dispatch arms have one fixed call shape
# but the candidate methods have different arities).

class T_poly_dispatch_arity_padding_Switch
  def initialize
    @v = 1
  end
  def toggle
    @v = 0
  end
  attr_reader :v
end

class T_poly_dispatch_arity_padding_Knob
  def toggle(soft = true)
    0
  end
end

class T_poly_dispatch_arity_padding_Box
  def initialize
    @items = [T_poly_dispatch_arity_padding_Switch.new, T_poly_dispatch_arity_padding_Switch.new]   # poly-typed; .toggle dispatch
                                        # over every class with `toggle`
  end
  attr_reader :items
end

b = T_poly_dispatch_arity_padding_Box.new
b.items[0].toggle
b.items[1].toggle
puts b.items[0].v   # 0
puts b.items[1].v   # 0

# === poly_dispatch_arity_truncate ===
# Poly-dispatch arms must match each candidate method's *fixed* C
# arity. The padding side (target takes more params than the call
# supplied — defaults fill the extras) was already in place. The
# truncate side is the complement: when a candidate class's method
# accepts *fewer* params than the call supplied, that arm has to
# drop the surplus to compile, even though the runtime cls_id check
# would skip it for a non-matching receiver.

class T_poly_dispatch_arity_truncate_Heater
  def initialize
    @v = 0
  end
  def write(addr, data)
    @v = addr + data
  end
  attr_reader :v
end

class T_poly_dispatch_arity_truncate_Buzzer
  def initialize
    @v = 0
  end
  def write(addr)        # one fewer arg than T_poly_dispatch_arity_truncate_Heater#write
    @v = addr
  end
  attr_reader :v
end

class T_poly_dispatch_arity_truncate_Box
  def initialize
    @poly = nil
    @poly = "x"
    @poly = T_poly_dispatch_arity_truncate_Heater.new   # widen to poly via heterogeneous writes
  end

  attr_reader :poly

  def call_write(addr, data)
    @poly.write(addr, data)   # poly recv → arms over T_poly_dispatch_arity_truncate_Heater + T_poly_dispatch_arity_truncate_Buzzer
  end
end

b = T_poly_dispatch_arity_truncate_Box.new
b.call_write(7, 5)
puts b.poly.v   # 12  (T_poly_dispatch_arity_truncate_Heater: addr + data)

