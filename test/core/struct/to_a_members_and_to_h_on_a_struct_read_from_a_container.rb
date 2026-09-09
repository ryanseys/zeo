# to_a is the member values and members the field names, on a value max_by
# picked out of an array.
# (spinel issue #2937)
E = Struct.new(:a, :b)
x = [E.new(1, 2), E.new(3, 4)].max_by(&:b)
p x.to_h
p x.to_a
p x.members
D = Data.define(:x, :y)
d = [D.new(1, 2), D.new(5, 6)].max_by(&:y)
p d.to_h
p d.members
__END__
{a: 3, b: 4}
[3, 4]
[:a, :b]
{x: 5, y: 6}
[:x, :y]
