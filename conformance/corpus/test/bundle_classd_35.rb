# Bundled tests:
#   - runtime_widen_int_to_poly_array
#   - sample_user_method
#   - scan_ivars_inherited_slot

# === runtime_widen_int_to_poly_array ===
# `compile_bracket_assign`'s `rt == "poly"` branch dispatches
# `@arr[i] = v` by runtime cls_id, but when the slot's runtime
# storage is IntArray and the rhs carries a non-int payload (a
# typed pointer / box whose cls_id we'd lose if cast to int),
# the write silently truncates to int.
#
# This patch widens the storage at runtime: allocate a fresh
# PolyArray, copy each existing IntArray element as sp_box_int,
# set the new slot, and reassign the slot expression to hold the
# new PolyArray.

class T_runtime_widen_int_to_poly_array_C
  def init_arr_widen
    @arr = [10, 20, 30]   # int_array storage
    @arr[1] = "string!"   # heterogeneous → triggers runtime widen
  end
  def init_str
    @arr = "scalar"       # forces slot type to widen to poly
  end
  def at(i); @arr[i]; end
end

c = T_runtime_widen_int_to_poly_array_C.new
c.init_arr_widen
puts c.at(0)               # 10 (preserved as int)
puts c.at(1)               # "string!" (recovered via widen)
puts c.at(2)               # 30

# === sample_user_method ===
# `infer_method_name_type` had a name-based shortcut for `sample`
# that returned the Array#sample default of int regardless of recv.
# A user-defined `sample` on an obj receiver inferred as int even
# though the method returns a non-int — and downstream consumers
# (puts, comparisons, etc.) used the wrong shape.

class T_sample_user_method_Box
  def initialize(label)
    @label = label
  end

  attr_reader :label

  def sample
    @label
  end
end

b = T_sample_user_method_Box.new("ok")
puts b.sample    # ok

# === scan_ivars_inherited_slot ===
# When a child class first writes to an ivar that already lives on
# a parent, scan_ivars used to register the ivar on the child too.
# update_ivar_type then recursed up to the parent and widened
# (e.g. `int` → `poly` for heterogeneous writes), but the child's
# own-table entry kept the new write's narrower type. Two tables
# disagreed about the slot — and downstream type lookups picked
# the wrong one depending on path, so codegen for the same field
# disagreed across class methods.
#
# Now scan_ivars detects that the slot is already in an ancestor's
# table and routes the write through update_ivar_type without
# adding a duplicate entry to the child. The parent's table stays
# the single source of truth.

class T_scan_ivars_inherited_slot_Parent
  def initialize
    @v = 0
    @v = "s"     # parent widens @v to poly
  end
end

class T_scan_ivars_inherited_slot_Child < T_scan_ivars_inherited_slot_Parent
  def initialize
    super
    @v = 42      # child writes through inherited slot
  end
  def read
    @v
  end
end

puts T_scan_ivars_inherited_slot_Child.new.read     # 42

