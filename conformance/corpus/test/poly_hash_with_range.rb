# Range crossing into a poly container: a hash literal mixing
# Integer values with Range values used to fall through
# `box_non_nullable_value_to_poly`'s default branch and emit
# `sp_box_int(sp_range_new(...))`, which the C compiler rejects
# (sp_Range is 16 bytes; sp_RbVal's union is 8). Same shape for
# heterogeneous poly arrays.
#
# Fix: heap-allocate the Range and box the pointer through a new
# SP_BUILTIN_RANGE cls_id (sp_box_range copies the stack range
# onto the GC heap before returning the boxed sp_RbVal).

# Hash form (the canonical Rails strong-status shape from the issue).
STATUS_SYMBOLS = {
  success:    200..299,
  redirect:   300..399,
  missing:    404,
  not_found:  404,
  error:      500..599,
  ok:         200,
  created:    201,
  no_content: 204,
}

puts STATUS_SYMBOLS[:success]    # 200..299
puts STATUS_SYMBOLS[:ok]         # 200
puts STATUS_SYMBOLS[:created]    # 201
puts STATUS_SYMBOLS[:error]      # 500..599
puts STATUS_SYMBOLS[:not_found]  # 404

# Array form — same fundamental issue (Range crosses into poly slot).
arr = [200..299, 200, 300..399, 404]
puts arr.length                  # 4
puts arr[0]                      # 200..299
puts arr[1]                      # 200
puts arr[2]                      # 300..399
puts arr[3]                      # 404
