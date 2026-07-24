# to_a / members / to_h on a Struct read out of a poly container (Struct#to_a
# is its member values, #members the field-name symbols).
E = Struct.new(:a, :b)
x = [E.new(1, 2), E.new(3, 4)].max_by(&:b)
p x.to_h
p x.to_a
p x.members
D = Data.define(:x, :y)
d = [D.new(1, 2), D.new(5, 6)].max_by(&:y)
p d.to_h
p d.members
