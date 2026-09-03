# Hash conformance (KieranP #2399,#2406-#2410,#2416,#2417)
p({} < { a: 1 })
p({ a: 1 } > {})
p({} <= {})
p({} >= { a: 1 })
p({}.min)
p({}.max)
p({}.minmax)
p({ a: 1, b: 2 }.invert.invert)
p({ a: 1 }.values_at)
p(Hash.new(0).values_at(:x, :y))
p(Hash.new(7).default(:x))
h = Hash.new(9)
p h.default(:zz)
p({}.to_h)
p({}.sum)
p({}.sum(5))
r = { a: 1, b: 2 }.each_with_index { |_pair, _i| nil }
p r
__END__
true
true
true
false
nil
nil
[nil, nil]
{a: 1, b: 2}
[]
[0, 0]
7
9
{}
0
5
{a: 1, b: 2}
