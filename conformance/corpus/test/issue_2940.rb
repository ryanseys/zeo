# Multiple assignment from #deconstruct of a Data/Struct value read out of a
# poly container: deconstruct is the member values (an Array), which the poly
# receiver derives via sp_poly_to_a_arr.
V = Data.define(:x, :y)
first = [V.new(1, 2)].first
a, b = first.deconstruct
p [a, b]
p first.deconstruct
S = Struct.new(:m, :n)
s = [S.new(5, 6), S.new(7, 8)].last
c, d = s.deconstruct
p [c, d]
