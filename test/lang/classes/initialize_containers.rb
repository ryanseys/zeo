# The container initialize family, in place -- oracle-pinned semantics
# including the corners: Hash#initialize touches ONLY the default channel,
# Set#initialize merges without a frozen check (a 4.0 CoreSet quirk).
def err(label)
  yield
  puts "#{label}: no error"
rescue => e
  puts "#{label}: #{e.class}: #{e.message}"
end

# ---- Array
a = [1, 2, 3]
p a.send(:initialize, 2, 0).equal?(a)
p a
a.send(:initialize, 3) { |i| i * 2 }
p a
a.send(:initialize, 2)
p a
a.send(:initialize)
p a
b = [1]
b.send(:initialize, [7, 8])
p b
err("ary frozen") { [1].freeze.send(:initialize, 2, 0) }
c = [1]
p c.send(:initialize_copy, [7, 8]).equal?(c)
p c
err("ary icopy type") { [1].send(:initialize_copy, "x") }

# ---- Hash
h = { a: 1 }
p h.send(:initialize, 9).equal?(h)
p [h, h[:missing]]
h2 = { a: 1 }
h2.send(:initialize) { |_, k| k.to_s }
p [h2, h2[:zz]]
err("hash frozen") { {}.freeze.send(:initialize) }
h3 = { a: 1 }
h3.send(:initialize_copy, { b: 2 })
p h3
h4 = {}
h4.send(:initialize_copy, Hash.new(5))
p h4[:zz]

# ---- String
s = +"abc"
p s.send(:initialize, "xyz").equal?(s)
p s
s.send(:initialize)
p s
s.send(:initialize, "q", encoding: "ASCII-8BIT")
p [s, s.encoding.name]
s2 = +"abc"
s2.send(:initialize, capacity: 100)
p s2
err("str frozen") { "abc".freeze.send(:initialize, "x") }
s3 = +"a"
s3.send(:initialize_copy, "zz")
p s3

# ---- Set (merges; skips the frozen check)
st = Set[1, 2]
st.send(:initialize, [7, 8])
p st
st2 = Set[1]
st2.send(:initialize, [1, 2]) { |x| x * 10 }
p st2
fr = Set[1].freeze
fr.send(:initialize, [2])
p fr
st3 = Set[1]
st3.send(:initialize_copy, Set[9])
p st3

# Ownership: every row sits on its own class.
%w[Array Hash String Set].each do |k|
  c = Object.const_get(k)
  print c.instance_method(:initialize).owner, " "
  puts c.instance_method(:initialize_copy).owner
end
__END__
true
[0, 0]
[0, 2, 4]
[nil, nil]
[]
[7, 8]
ary frozen: FrozenError: can't modify frozen Array: [1]
true
[7, 8]
ary icopy type: TypeError: no implicit conversion of String into Array
true
[{a: 1}, 9]
[{a: 1}, "zz"]
hash frozen: FrozenError: can't modify frozen Hash: {}
{b: 2}
5
true
"xyz"
"xyz"
["q", "ASCII-8BIT"]
""
str frozen: FrozenError: can't modify frozen String: "abc"
"zz"
Set[1, 2, 7, 8]
Set[1, 10, 20]
Set[1, 2]
Set[9]
Array Array
Hash Hash
String String
Set Set
