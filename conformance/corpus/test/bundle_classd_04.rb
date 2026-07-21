# Bundled tests:
#   - instance_eval_return_value
#   - map_array_block_result
#   - map_block_returns_nested_array

# === instance_eval_return_value ===
# instance_eval as an expression: the block's last expression is the
# call's value (CRuby semantics). Void-bodied blocks (assignment-only)
# fall through to a truthy receiver so `if obj.instance_eval { @f = 1 }`
# still works.

class T_instance_eval_return_value_Counter
  def initialize
    @count = 0
  end
  def inc
    @count = @count + 1
  end
end

c = T_instance_eval_return_value_Counter.new
c.inc
c.inc
c.inc

# Int-valued block returns the ivar, not the receiver.
n = c.instance_eval { @count }
puts n   # 3

# Boolean-valued block.
big = c.instance_eval { @count > 2 }
puts big # true

# Arithmetic expression body.
doubled = c.instance_eval { @count + @count }
puts doubled # 6

# String-valued block (pointer-typed return).
class T_instance_eval_return_value_Greeter
  def initialize
    @greeting = "hello"
  end
end

g = T_instance_eval_return_value_Greeter.new
msg = g.instance_eval { @greeting }
puts msg # hello

# Void body: receiver flows out via comma-expr fallback, keeping the
# `if` arm truthy.
class T_instance_eval_return_value_Flag
  def initialize
    @flag = 0
  end
end

f = T_instance_eval_return_value_Flag.new
if f.instance_eval { @flag = 1 }
  puts "truthy"
end

puts "done"

# === map_array_block_result ===
# `arr.map { array_block }` — outer recv is a typed Array, block
# returns a typed array (`(0..i).to_a` here is int_array). Without
# the fix, two issues conspire:
#
#   - infer_method_name_type returned int_array, so the ivar `@rows`
#     was typed as int_array. The outer accumulator was an
#     sp_IntArray pushed via sp_IntArray_push — but each iteration's
#     value was an sp_IntArray pointer reinterpreted as mrb_int,
#     which gcc rejects on -Wint-conversion.
#   - Even if the cast were silent, the inner arrays would not be
#     traced from an IntArray slot and could be collected mid-loop.
#
# After the fix, the result is `<elem>_ptr_array` and the codegen
# accumulator is an sp_PtrArray pushed via sp_PtrArray_push.

class T_map_array_block_result_C
  def initialize
    @rows = [1, 6].map { |i| (0..i).to_a }
  end

  def show
    @rows.each do |row|
      row.each { |x| print x, " " }
      puts
    end
  end
end

T_map_array_block_result_C.new.show

# === map_block_returns_nested_array ===
# `Range#map` / `int_array.map` whose block returns an array
# (1D, 2D, 3D nested) used to silently degrade — the codegen
# returned `0` for non-int/string/float block returns at the
# Range#map path, and `int_array.map` boxed inner ptr_array
# results via `sp_box_ptr_array(val)` which erased the
# elem-type info (cls_id PTR_ARRAY only) so deeper indexing
# fell through every dispatch arm to `sp_box_nil()`.
#
# Fixes:
#
# - Range#map block returning a typed array now stores in
#   `sp_PtrArray` (matching the inferred `<bret>_ptr_array`
#   type). Deeper nesting (block returns ptr_array /
#   poly_array) stores in `sp_PolyArray` with `box_value_to_poly`
#   boxing — preserves cls_id chain for `arr[i][j][k]...`.
#
# - `int_array.map`'s deep-array branch (block returns
#   `<X>_ptr_array`) converts the inner sp_PtrArray to
#   sp_PolyArray inline via a per-element `sp_box_<elem>` re-tag,
#   so the outer PolyArray's elements carry the right cls_id at
#   the next dispatch level.
#
# Together this lets nested-map-of-array-block constructs
# (e.g. optcarrot's `TILE_LUT = [...].map { (0..7).map {
# (0...0x10000).map { ... } }.transpose }`) compile and read
# at every depth.

# Range#map -> int_array_ptr_array (2D)
class T_map_block_returns_nested_array_M2
  def initialize
    @t = (0..2).map { |i| (0..3).map { |j| i * 10 + j } }
  end
  def read(i, j); @t[i][j]; end
end
m2 = T_map_block_returns_nested_array_M2.new
puts m2.read(0, 0)   # 0
puts m2.read(1, 2)   # 12
puts m2.read(2, 3)   # 23

# int_array.map { Range#map { ... } } -> 3D via PolyArray
class T_map_block_returns_nested_array_M3
  def initialize
    @t = [10, 20].map do |a|
      (0..2).map { |b| [a + b, a + b + 100] }
    end
  end
  def read(i, j, k); @t[i][j][k]; end
end
m3 = T_map_block_returns_nested_array_M3.new
puts m3.read(0, 0, 0)   # 10
puts m3.read(0, 1, 1)   # 111
puts m3.read(1, 2, 0)   # 22
puts m3.read(1, 2, 1)   # 122

