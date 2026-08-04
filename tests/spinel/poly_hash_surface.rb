# The Hash/Enumerable surface on a value that arrives boxed: a seedless
# Array#reduce result, a Fiber#resume value, a container read. The receiver's
# runtime class is unknown at the call site, so each line pins either an arm
# that pretends the receiver is the general boxed hash, or one that branches on
# the runtime kind. The array cases below guard the second kind: a name Array
# also owns must keep answering Array's way.
def fv(x)
  f = Fiber.new { Fiber.yield(x); nil }
  f.resume
end

h = fv({ n: 1, s: 2 })

p h.dig(:n)
p h.value?(1)
p h.invert
p h.assoc(:n)
p h.rassoc(1)
p h.filter_map { |k, v| k }
p h.each_with_object([]) { |(k, v), a| a << k }
p h.each_with_object({}) { |(k, v), a| a[v] = k }
p h.group_by { |k, v| v }
p h.partition { |k, v| v == 1 }
p h.zip([9, 8])
p h.reduce(0) { |a, (k, v)| a + v }
p h.find { |k, v| k == :n }
p h.take(1)
p h.drop(1)
p h.select { |k, v| k == :n }
p h.reject { |k, v| k == :n }
p h.slice(:n)
p h.slice(:n, :s)
p h.except(:n)
p h.values_at(:n, :s)
p h.fetch_values(:n)
p h.entries
p h.flatten
p h.first
p h.each_pair.to_a
p h.each_key.to_a
p h.each_value.to_a
p h.each_entry.to_a
p h.transform_values { |v| v * 2 }
p h.transform_keys { |k| k.to_s }
p h.tally.size
p h.chunk_while { |a, b| true }.to_a
p h.flat_map { |k, v| [k] }
p h.none? { |k, v| v > 5 }
p h.any? { |k, v| v > 1 }
p h.all? { |k, v| v > 0 }
p h.one? { |k, v| v == 1 }
p h.count { |k, v| v > 1 }
p h.to_h
p fv({ n: 1, s: nil }).compact

# the seedless Array#reduce shapes the reports were filed against
p [{ n: 1 }].reduce { |acc, l| acc }.first
p [{ n: 1 }, { m: 2 }].reduce { |acc, l| acc.merge(l) }.to_h

# a boxed Array answers the shared names on its own terms
a = fv([1, 2, 3])
p a.select { |x| x > 1 }
p a.reject { |x| x > 1 }
p a.first
p a.last
p a.slice(1, 2)
p a.slice(0)
p a.compact
p a.flatten
p a.find { |x| x > 1 }
p a.flat_map { |x| [x, x] }
p a.count { |x| x > 1 }
p a.none? { |x| x > 5 }
p a.chunk_while { |x, y| y == x + 1 }.to_a

# and a boxed String keeps the names it shares with them
s = fv("abcdef")
p s.slice(1, 2)
p s.slice(1)
p s.count("a")
p s.index("c")
