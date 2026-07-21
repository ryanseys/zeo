# Enumerable#inject / #reduce with a symbol-to-proc argument
# (`inject(&:&)`) and the seedless first-element fold.
#
# `inject(&:sym)` on a non-int_array receiver reached compile_reduce_block
# with a synthetic accumulator `_acc` that was never declared in C
# (`'lv__acc' undeclared`). The seedless fold also seeded the accumulator
# with `0`/NULL and iterated from index 0 instead of seeding with the
# first element and iterating from index 1 — wrong for non-additive ops
# (`*` gave 0) and a NULL deref for array elements (`a & b` segfaulted).
#
# Fix: seed seedless folds with arr[0] (loop from 1), type the result as
# the element type in analyze, declare the synthetic `_acc`, and lower
# `&:sym` / `:sym` to `acc.sym(x)` for the modeled element/op pairs.

# 1. Symbol-to-proc set operations on an array of int arrays.
p [[1, 2, 3], [2, 3, 4]].inject(&:&)        #=> [2, 3]
p [[1, 2], [2, 3], [3, 4]].inject(&:|)      #=> [1, 2, 3, 4]
p [[1, 2, 3, 4], [2, 3]].inject(&:-)        #=> [1, 4]

# 2. Single-element array: the fold returns that element (loop body
#    never runs).
p [[5, 6, 7]].inject(&:&)                   #=> [5, 6, 7]

# 3. Symbol-to-proc arithmetic on int arrays (folds inline upstream).
p [1, 2, 3, 4].inject(&:+)                  #=> 10
p [1, 2, 3, 4].inject(&:*)                  #=> 24

# 4. Seedless explicit-block fold seeds with the first element, not 0.
p [1, 2, 3, 4].inject { |a, b| a * b }      #=> 24
p [10, 3, 2].inject { |a, b| a - b }        #=> 5

# 5. Seedless explicit-block fold over an array of arrays (was a NULL
#    deref segfault when the accumulator seeded as NULL).
p [[1, 2, 3], [2, 3, 4]].inject { |a, b| a & b }   #=> [2, 3]

# 6. Result flows through a local, exercising the inferred result type.
inter = [[1, 2, 3], [2, 3, 4], [3, 2, 5]].inject(&:&)
puts inter.length                           #=> 2
p inter                                     #=> [2, 3]

puts "done"
