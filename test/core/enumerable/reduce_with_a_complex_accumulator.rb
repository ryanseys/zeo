# A reduce seeded with Complex(0, 0) folding + over Complex elements, and the
# same with * .
zs = [Complex(1, 2), Complex(3, -4)]
total = zs.reduce(Complex(0, 0)) { |acc, z| acc + z }
p total
ws = [Complex(2, 0), Complex(0, 3), Complex(1, 1)]
prod = ws.reduce(Complex(1, 0)) { |acc, z| acc * z }
p prod
__END__
(4-2i)
(-6+6i)
