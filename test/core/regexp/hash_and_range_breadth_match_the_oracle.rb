# The Hash + Range Tier A surface: merge (with conflict block), fetch
# shapes, dig, invert/key/value?, filters + transforms, each_key/value,
# Range size/step/last(n), and String-range iteration via succ.

h = { a: 1, b: 2 }
p h.merge({ c: 3 })
p h.merge({ a: 9 }) { |k, old, new| old + new }
p h.to_a
p h.invert
p h.key(2)
p h.key(9)
p h.fetch(:a)
p h.fetch(:x, 0)
begin
  h.fetch(:x)
rescue KeyError => e
  puts "KeyError: #{e.message}"
end
p(h.fetch(:x) { |k| "no #{k}" })
p(h.select { |k, v| v > 1 })
p(h.reject { |k, v| v > 1 })
p(h.transform_values { |v| v * 10 })
p(h.any? { |k, v| v > 1 })
p h.count
p(h.min_by { |k, v| v })
p h.value?(2)
p h.value?(9)
p({ x: { y: 5 } }.dig(:x, :y))
h2 = { a: 1 }
h2.update({ b: 2 })
p h2
acc = []
h.each_key { |k| acc << k }
h.each_value { |v| acc << v }
p acc
p h == { b: 2, a: 1 }
p h == { a: 1 }
r = (1..10)
p r.sum
p r.min
p r.max
p r.count
p r.size
p r.first(3)
p r.last(3)
p (1...5).size
acc2 = []
(1..10).step(3) { |i| acc2 << i }
p acc2
p ("a".."e").to_a
p ("a".."e").include?("c")
__END__
{a: 1, b: 2, c: 3}
{a: 10, b: 2}
[[:a, 1], [:b, 2]]
{1 => :a, 2 => :b}
:b
nil
1
0
KeyError: key not found: :x
"no x"
{b: 2}
{a: 1}
{a: 10, b: 20}
true
2
[:a, 1]
true
false
5
{a: 1, b: 2}
[:a, :b, 1, 2]
true
false
55
1
10
10
10
[1, 2, 3]
[8, 9, 10]
4
[1, 4, 7, 10]
["a", "b", "c", "d", "e"]
true
