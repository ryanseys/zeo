# Phase 17.1 -- Tier A breadth across String/Symbol/Array/Hash/Range/
# Enumerable/Kernel, resolved down the CRuby-exact ancestor chains
# (Comparable driving <=>, Enumerable driving each, Kernel's universals).

p "hello world".capitalize
p "  hi  ".strip
p "a,b,,c".split(",")
p "hello".gsub("l") { |m| m.upcase }
p "hello".tr("a-y", "b-z")
p "az".succ
p "Hello %s, you are %d" % ["Bob", 42]
p "hi".center(7, "*")
p :hello.length
p [3, 1, 2].map(&:to_s)

p "abc" < "abd"
p "m".clamp("a", "f")
p 5.between?(1, 10)

p [1, 2] | [2, 3]
p [1, [2, [3]]].flatten
p [1, 2, 2, 3, 1].uniq
p [1, 2, 3].join("-")
p [[1, [2, 3]]].dig(0, 1, 0)
p [1, 2, 3].zip([4, 5, 6])
p [0, 1, 2, 3, 4][1..3]
p [3, 1, 2].sort { |a, b| b <=> a }

h = { a: 1, b: 2 }
p h.merge({ a: 9 }) { |k, old, new| old + new }
p h.invert
p h.transform_values { |v| v * 10 }
p h.select { |k, v| v > 1 }
p({ x: { y: 5 } }.dig(:x, :y))

p (1..6).group_by { |x| x % 3 }
p [1, 2, 3, 4].partition { |x| x.even? }
p ["a", "b", "a"].tally
p [1, 2, 3].each_with_object([]) { |x, memo| memo << x * 10 }
acc = []
(1..10).step(3) { |x| acc << x }
p acc
p ("a".."e").to_a

puts [1, [2, nil]], "done"
r = p 1, "two"
p r
caught = catch(:stop) do
  [5, 6, 7].each { |x| throw :stop, x * 2 if x == 6 }
end
p caught

case 42
when 1..50 then puts "in range"
end
p [1, 2].respond_to?(:map)
p 5.is_a?(Comparable)
