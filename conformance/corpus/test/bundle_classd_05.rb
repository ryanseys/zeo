# Bundled tests:
#   - map_empty_block
#   - map_range_recv_array_block

# === map_empty_block ===
# Issue #210 follow-up: an empty `map {}` block must still produce
# an array of the same length as the receiver. Each map dispatch
# (Range#, N.times#, int_array#, str_array#, poly_array#) used to
# skip the push entirely on an empty block, leaving the typed
# accumulator short and downstream `.length` / `[i]` wrong.

# Range#map
puts (0..2).map {}.length          # 3

# N.times.map
puts 4.times.map {}.length         # 4

# int_array#map
puts [10, 20, 30].map {}.length    # 3

# str_array#map
puts ["a", "b", "c", "d"].map {}.length   # 4

# poly_array#map
puts [1, "two", :three].map {}.length     # 3

# ptr_array#map (homogeneous obj_X[])
class T_map_empty_block_Box
  def initialize(v); @v = v; end
end
puts [T_map_empty_block_Box.new(1), T_map_empty_block_Box.new(2), T_map_empty_block_Box.new(3)].map {}.length  # 3

# === map_range_recv_array_block ===
# `infer_method_name_type` for `map` used to fall through to
# `infer_type(recv)` when the block returned a non-trivial shape
# (an array literal). For a Range recv that yielded `range`,
# poisoning any ivar holding the result; an `@x = something_else`
# assignment then failed to type-check (`@x = 0` against an
# `sp_Range` slot).
#
# Now `Range#map { array_block }` infers as `<inner>_ptr_array`
# (matching the runtime sp_PtrArray storage), so the result can
# be indexed `arr[i][j]` directly. A subsequent re-assignment
# with a different array shape goes to a separate slot to avoid
# spinel's slot widening to poly.

class T_map_range_recv_array_block_C
  def initialize
    # Block returns an int_array — outer is int_array_ptr_array.
    @rows = (0...3).map { |i| [i, i * 10] }
    # Separate slot for a plain int_array.
    @flat = [10, 20, 30]
  end

  def show
    @rows.each { |row| row.each { |v| puts v } }
    @flat.each { |v| puts v }
  end
end

T_map_range_recv_array_block_C.new.show
# Expected:
#   0 / 0 / 1 / 10 / 2 / 20 (each row's elements)
#   10 / 20 / 30 (flat)

