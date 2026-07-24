# promote-only: an accumulator multiplied inside a block-iteration loop grows
# past 64 bits. In --int-overflow=promote the analyzer widens it to bigint
# (the same self-referential-multiply detection used for `while`, extended to
# block loops); in raise/wrap mode this would raise, so the test is filtered
# out of those runs by the Makefile.

# .times block, `acc = acc * x` form
f = 1
25.times { |i| f = f * (i + 1) }
puts f

# range#each block
g = 1
(1..30).each { |k| g = g * 2 }
puts g

# .times block, operator-assign on a captured accumulator (bigint cell)
h = 1
40.times { h *= 3 }
puts h
