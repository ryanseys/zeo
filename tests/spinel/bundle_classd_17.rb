# Bundled tests:
#   - poly_aref_shift_via_block_param
#   - poly_array_literal_callnode_typed_elem
#   - poly_array_slot_literal
#   - poly_array_slot_sized_default
#   - poly_dispatch_args_ret

# === poly_aref_shift_via_block_param ===
# `<poly>[idx]` flowing into an int operator (`>>` / `<<` / ...)
# inside an `.each {|a| ... }` block must unbox the sp_RbVal result
# of `a[idx]` before the T_poly_aref_shift_via_block_param_C operator.
#
# `entries` here is one half of a destructured 2-tuple from a
# nested-ivar chain (`@store[bank][idx]`), which widens it to poly.
# The block param `a` is then poly (not poly_array), so `a[2]`
# emits via compile_poly_method_call's runtime tag-check → sp_RbVal.
# Pre-fix, the analyze cache for `a[2]` sat at "int" (one observed
# elem kind), so `compile_arg0_as_int` for `data >> a[2]` skipped
# the unbox and emitted `lv_data >> _t.sp_RbVal` into the T_poly_aref_shift_via_block_param_C —
# `invalid operands to binary >>`. The fix in infer_type matches
# the actual emit so the unbox lands.

class T_poly_aref_shift_via_block_param_C
  def initialize
    # @store[bank][idx] is a 2-tuple: (names, entries).  Each entry
    # in `entries` is a [tag, payload, shift] triple; the
    # heterogeneous element shape forces poly typing through the
    # destructure.
    @store = [[ [[1], [[10, [0, 1, 2], 4], [20, [3, 4, 5], 6]]] ]]
  end

  def run(bank, idx, data)
    _names, entries = @store[bank][idx]
    entries.each do |a|
      shifted = data >> a[2]
      puts shifted & 3
    end
  end
end

T_poly_aref_shift_via_block_param_C.new.run(0, 0, 0xff)

# === poly_array_literal_callnode_typed_elem ===
# `[A, B]` outer literal where A and B are non-ArrayNode expressions
# returning a typed `<X>_ptr_array` (e.g. `(0..N).map { ... }`).
# Without the fix, compile_array_literal's poly_array branch hits
# `box_value_to_poly(et, val)` for the CallNode element, which lowers
# to `sp_box_ptr_array(val)` — cls_id PTR_ARRAY only, the inner
# element type is erased. Subsequent `arr[b][i][j]` reads dispatch
# through the PTR_ARRAY arm at level 1, which boxes the next-level
# element as `sp_box_obj(p, 0)` (cls_id 0, "unknown obj"), and the
# level-2 dispatch can't match any cls_id arm — every leaf read
# returns 0.
#
# With the fix, the runtime conversion loop iterates the typed
# PtrArray and pushes each inner with `sp_box_<inner-type>` (e.g.
# `sp_box_int_array(...)` for an IntArray element), so the cls_id
# chain stays tagged through every level.

class T_poly_array_literal_callnode_typed_elem_C
  def initialize
    @a = [(0..3).map { |i| [i * 10, i * 10 + 1, i * 10 + 2] },
          (0..3).map { |i| [i * 100, i * 100 + 1, i * 100 + 2] }]
  end
  def get(b, i, j); @a[b][i][j]; end
  def out_len; @a.length; end
  def mid_len(b); @a[b].length; end
  def in_len(b, i); @a[b][i].length; end
end

c = T_poly_array_literal_callnode_typed_elem_C.new
puts c.out_len           # 2
puts c.mid_len(0)        # 4
puts c.in_len(0, 0)      # 3
puts c.get(0, 0, 0)      # 0
puts c.get(0, 1, 1)      # 11
puts c.get(0, 3, 2)      # 32
puts c.get(1, 0, 0)      # 0
puts c.get(1, 2, 1)      # 201
puts c.get(1, 3, 2)      # 302

# === poly_array_slot_literal ===
# `@arr = [a, b]` going into a poly_array slot used to compile the
# rhs via `compile_array_literal`, which infers a typed storage
# (ptr_array of one class for homogeneous obj literals, etc.) and
# emits the matching `sp_PtrArray *`. The slot is `sp_PolyArray *`
# (widened by writer-scan via heterogeneous `@arr[i] = v` writes
# elsewhere in the class), so the resulting C contains a pointer-
# type mismatch:
#
#   sp_PtrArray *_t1 = sp_PtrArray_new();
#   sp_PtrArray_push(_t1, sp_Pad_new());
#   self->iv_pads = _t1;          /* iv_pads is sp_PolyArray * */
#
# The default `-Wno-all` build accepts the cast silently and
# coerces, but strict flags reject it as
# `assignment to 'sp_PolyArray *' from incompatible pointer type
# 'sp_PtrArray *'`. Even under `-Wno-all` the runtime is wrong:
# `PolyArray_set` writes 16-byte sp_RbVal entries into 8-byte
# PtrArray slots and silently corrupts adjacent memory.

class T_poly_array_slot_literal_Pad
  def initialize(n)
    @n = n
    @arr = []
    @arr << n
  end
  attr_reader :n
end

class T_poly_array_slot_literal_Holder
  def initialize
    @pads = [T_poly_array_slot_literal_Pad.new(10), T_poly_array_slot_literal_Pad.new(20)]
    @pads[0] = "x"   # heterogeneous []= → @pads widens to poly_array
  end
  attr_reader :pads

  def count
    @pads.length
  end
end

puts T_poly_array_slot_literal_Holder.new.count    # 2

# === poly_array_slot_sized_default ===
# `@arr = [nil] * N` going into a poly_array slot used to lower
# the rhs via the default `*` codegen, which produces an sp_IntArray.
# The slot is `sp_PolyArray *` (widened by writer-scan via
# heterogeneous `@arr[i] = v` writes elsewhere in the class), so
# the resulting C contains a pointer-type mismatch:
#
#   sp_IntArray *_t1 = sp_IntArray_new();
#   ...
#   self->iv_arr = _t1;          /* iv_arr is sp_PolyArray * */
#
# The default `-Wno-all` build silently coerces and the runtime
# runs with garbage: subsequent `sp_PolyArray_set(@arr, ...)`
# calls write 16-byte sp_RbVal entries into 8-byte IntArray slots,
# corrupting adjacent memory and skewing `@arr.length`.
#
# An equivalent guard exists for ptr_array slots; this PR extends
# it to poly_array slots (constructor + general InstanceVariable-
# WriteNode paths) so the lowered storage matches the slot.

class T_poly_array_slot_sized_default_Holder
  def initialize
    @arr = [nil] * 3
    @arr[0] = 42      # int
    @arr[1] = "two"   # str — heterogeneous → @arr widens to poly_array
  end
  attr_reader :arr

  def count
    @arr.length
  end
end

puts T_poly_array_slot_sized_default_Holder.new.count   # 3

# === poly_dispatch_args_ret ===
# Regression test for polymorphic return flow:
# a method returning different class instances from branches.

class T_poly_dispatch_args_ret_A
  def read(v)
    v + 1
  end
end

class T_poly_dispatch_args_ret_B
  def read(v)
    v + 2
  end
end

class T_poly_dispatch_args_ret_Builder
  def make(flag)
    if flag == 0
      T_poly_dispatch_args_ret_A.new
    else
      T_poly_dispatch_args_ret_B.new
    end
  end

  def make_with_return(flag)
    if flag == 0
      return T_poly_dispatch_args_ret_A.new
    end
    return T_poly_dispatch_args_ret_B.new
  end
end

b = T_poly_dispatch_args_ret_Builder.new

obj0 = b.make(0)
puts obj0.read(41)

obj1 = b.make(1)
puts obj1.read(41)

ret0 = b.make_with_return(0)
puts ret0.read(41)

ret1 = b.make_with_return(1)
puts ret1.read(41)

