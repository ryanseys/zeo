# `v&.<enumerable> { }` on a nil receiver answers nil. The element-loop
# lowerings walk the receiver without looking at the call operator, so they ran
# on the very nil the guard exists to stop.
def pick(n) = n > 0 ? [1, 2, 4, 5] : nil

def probe(v)
  [v&.map { |x| x },
   v&.flat_map { |x| x },
   v&.select { |x| x > 2 },
   v&.reject { |x| x > 2 },
   v&.filter_map { |x| x if x > 2 },
   v&.partition { |x| x > 2 },
   v&.group_by { |x| x > 2 },
   v&.sort_by { |x| -x },
   v&.min_by { |x| -x },
   v&.count { |x| x > 2 },
   v&.sum { |x| x },
   v&.each { |x| x },
   v&.each_with_index { |x, i| x }]
end

p probe([1, 2, 4, 5])
p probe(nil)
