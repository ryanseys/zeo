# `c.with(a: c.a + 1)` inside a map, and with two members.
# (spinel issue #2890)
C = Data.define(:a)
r = [C.new(1), C.new(2)]
p r.map { |c| c.with(a: c.a + 1) }.map(&:a)

D = Data.define(:a, :b)
ds = [D.new(1, "x"), D.new(2, "y")]
p ds.map { |d| d.with(a: d.a * 10) }.map(&:a)
p ds.map { |d| d.with(b: d.b.upcase).to_h }
__END__
[2, 3]
[10, 20]
[{a: 1, b: "X"}, {a: 2, b: "Y"}]
