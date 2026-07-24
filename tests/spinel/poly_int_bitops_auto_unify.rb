# Same operator-site auto-unbox, but for bit operators (`<<`,
# `>>`, `&`, `|`, `^`). int recv + poly arg previously failed C
# compile; the poly arg is now unboxed via .v.i.

# Heterogeneous poly_array of [user-class-with-[], IntArray] —
# the outer `arr[i]` dispatch returns poly, then `[j]` on the
# poly returns poly (since spinel statically types `[]` as poly).
class Bits
  def [](i); [0xff, 0x0a, 0x10][i]; end
end

arr = [Bits.new, [0xff, 0x0a, 0x10]]

# Each `arr[k][j]` is `int + poly` style: addr is int, the
# heterogeneous dispatch result is statically poly.
puts(1   << arr[1][1])     # 1 << 0x0a = 1024
puts(0xf0 & arr[1][0])     # 0xf0 & 0xff = 0xf0 = 240
puts(0x05 | arr[1][1])     # 0x05 | 0x0a = 0x0f = 15
puts(0xff ^ arr[1][2])     # 0xff ^ 0x10 = 0xef = 239
puts(arr[1][1] >> 1)       # 0x0a >> 1 = 5  (recv-poly + arg-int already routes to sp_poly_shr)
