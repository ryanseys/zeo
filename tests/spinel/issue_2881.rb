# reduce with a Complex initial accumulator, folding over a poly array: the
# block's `acc + z` (z a Complex read out of the poly array, hence poly) keeps
# the accumulator typed Complex by coercing the poly operand.
zs = [Complex(1, 2), Complex(3, -4)]
total = zs.reduce(Complex(0, 0)) { |acc, z| acc + z }
p total
ws = [Complex(2, 0), Complex(0, 3), Complex(1, 1)]
prod = ws.reduce(Complex(1, 0)) { |acc, z| acc * z }
p prod
