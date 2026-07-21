# `arr[i] = v` in *expression* position on int_array. The stmt-
# form is handled by `compile_bracket_assign`; in rvalue chains
# like `outer = arr[i] = rhs`, the call lands in the expression
# dispatcher and was previously unresolved (warning + literal 0).
#
# Surfaced via optcarrot PPU's `@io_latch = @sp_ram[@regs_oam] = io_latch_mask(data)`
# shape — the `sp_ram[idx] = mask` write is also the rvalue
# assigned to `@io_latch`.

a = [10, 20, 30, 40, 50]

# Chained assignment: `b` should pick up the rhs (Ruby []= returns rhs).
b = a[2] = 999

puts a[0]
puts a[1]
puts a[2]   # mutated
puts a[3]
puts a[4]
puts b      # 999

# Index/value side-effect ordering: each subexpression evaluated once.
i = 0
def bump_i_to_3
  3
end

c = a[bump_i_to_3] = 7777
puts a[3]
puts c
