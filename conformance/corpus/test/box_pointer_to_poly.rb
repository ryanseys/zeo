# `box_val_to_poly` and `box_expr_to_poly` previously fell through
# to `sp_box_int(val)` for any pointer-typed expression they didn't
# explicitly recognize (hashes, mutable strings, etc.), yielding
# -Wint-conversion errors and silently truncating the pointer to a
# 64-bit int. The fix routes pointer types through `sp_box_obj`.
#
# Triggers the path: a method whose return type is widened to poly
# across branches (here: hash and string). The hash branch's
# `return {a: 1, ...}` runs through `box_expr_to_poly`, which
# without the fix emitted `sp_box_int(<sp_SymIntHash *>)` and gcc
# rejected with "passing argument 1 of sp_box_int makes integer
# from pointer without a cast".
#
# The test only verifies the program compiles and runs — round-
# tripping the hash back out of the poly slot is the caller's
# problem since the cls_id of 0 erases the concrete type.

def get(flag)
  if flag > 0
    return {a: 1, b: 2, c: 3}
  end
  "hello"
end

x = get(0)
y = get(1)
puts "ok"
