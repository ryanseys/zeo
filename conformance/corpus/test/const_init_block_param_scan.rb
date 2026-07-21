# A constant initializer's RHS can introduce block params (here
# `n`) whose declarations need to land at main()'s frame. The
# top-level-stmt scan in `emit_main` previously skipped const-init
# expressions, so iterations whose body assigns `lv_n` without a
# preceding `mrb_int lv_n;` (e.g. the `Array#sum { |n| ... }`
# emit path uses `emit_iter_open`, which does plain `elem_var =
# idx_var;`) failed C compilation with "undeclared identifier".

TOTAL = [1, 2, 3].sum { |n| n * 2 }
puts TOTAL                         # 12

DOUBLED = [4, 5, 6].sum { |m| m * 3 }
puts DOUBLED                       # 45
