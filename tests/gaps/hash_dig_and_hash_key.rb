# GAP -- imported from the spinel corpus at c55d9bdb.
# A Hash#dig walk through a self-referential structure recurses without
# bound and overflows the runtime stack.
#
# A dig step that lands on something with no #dig is a TypeError, not a quiet
# nil: only nil ends the walk. And a Hash hashes by content, so two `==` hashes
# agree -- it was hashing by pointer, which no two literals share.
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
