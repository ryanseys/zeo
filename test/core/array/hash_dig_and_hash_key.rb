# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# `a << a` -- a self-referential container, built on purpose to prove the walk terminates.
#@ gccheck: cycle leak: 2 objects (Array x1, Hash x1)
h = { a: 1 }
r = (h.dig(:a, :b) rescue $!.class); p r
r = (h.dig(:a, :b) rescue $!.message); p r
r = ({ a: "str" }.dig(:a, :b) rescue $!.class); p r

# the walk still ends quietly at a missing key
p h.dig(:zz)
p h.dig(:zz, :deeper)

# and still walks what it can
p({ a: { b: { c: 7 } } }.dig(:a, :b, :c))
p({ a: [10, 20] }.dig(:a, 1))
p({ a: { b: [1, { c: 2 }] } }.dig(:a, :b, 1, :c))

# Hash#hash is content-based and order-independent
p({ a: 1 }.hash == { a: 1 }.hash)
p({ a: 1, b: 2 }.hash == { b: 2, a: 1 }.hash)
p({ a: 1 }.hash == { a: 2 }.hash)
p({ "x" => [1, 2] }.hash == { "x" => [1, 2] }.hash)
p({}.hash == {}.hash)
p({ a: 1 }.eql?({ a: 1 }))

# self-referential containers terminate (CRuby answers a fixed value for the
# recursive reference; the walk is depth-capped here)
h2 = {}
h2[:me] = h2
p(h2.hash == h2[:me].hash)
a = []
a << a
p(a.hash == a[0].hash)
__END__
TypeError
"Integer does not have #dig method"
TypeError
nil
nil
7
20
2
true
true
false
true
true
true
true
true
