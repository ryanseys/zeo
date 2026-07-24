# reduce/inject with a &proc block argument forwards through a literal
# two-param block; the fold emitter keeps the proc call's prelude inside the
# loop, so every iteration publishes its own arguments (#2684).
pr = ->(a, x) { a + x }
p([1, 2, 3, 4].reduce(&pr))
p([1, 2, 3, 4].reduce(0, &pr))
p([1, 2, 3, 4].inject(10, &->(a, x) { a + x }))
mul = ->(a, x) { a * x }
p([1, 2, 3, 4].inject(&mul))
# the manual form the desugar lowers to (was returning 0 before the
# fold-prelude fix)
add = ->(a, b) { a + b }
p [1, 2, 3].reduce { |a, b| add.call(a, b) }
p [1, 2, 3].inject(10) { |a, b| add.call(a, b) }
# string accumulator
cat = ->(a, s) { a + s }
p(["x", "y", "z"].reduce(&cat))
