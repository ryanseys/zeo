# The Enumerable methods on a Range that do not collect as they go --
# reduce(:sym), group_by, find, count, zip and tally. The
# range is a local so the materialization runs at runtime.
r = (1..5)

# reduce / inject with a symbol or an initial value
p r.reduce(:+)
p r.inject(:*)
p r.reduce(100, :+)
p r.inject(2, :*)

# group_by returns a Hash of arrays
p (1..6).group_by { |x| x % 3 }

# find / detect return the first matching element (or nil)
p r.find { |x| x > 3 }
p r.detect(&:even?)
p r.find { |x| x > 99 }

# count with a block or an argument (bare count stays Range#size)
p r.count(&:even?)
p r.count(3)
p r.count

# zip pairs with other collections
p (1..3).zip([4, 5, 6])
p (1..3).zip([4, 5, 6], [7, 8, 9])

# tally counts occurrences
p (1..3).tally

# exclusive ranges exercise the `last - excl` materialization bound
x = (1...5)
p x.reduce(:+)
p x.group_by { |n| n % 2 }
p x.count(&:even?)
p x.zip([10, 20, 30, 40])

# empty ranges (first > last) materialize to an empty int array. reduce with an
# initial value returns that value; the no-init `reduce(:sym)` form on an empty
# array is the pre-existing typed-array limitation (returns the element-type
# identity, not nil, since the result is a non-nullable int) and is not tested.
e = (5..1)
p e.reduce(100, :+)
p e.group_by { |n| n }
p e.find { |n| n > 0 }
p e.count(&:even?)
p e.tally

# zip with a shorter argument pads the tail with nil
p (1..3).zip([4, 5])

# the natively-handled range methods are unchanged
p r.map { |x| x * 2 }
p r.select(&:odd?)
p r.sum
p r.to_a
__END__
15
120
115
240
{1 => [1, 4], 2 => [2, 5], 0 => [3, 6]}
4
2
nil
2
1
5
[[1, 4], [2, 5], [3, 6]]
[[1, 4, 7], [2, 5, 8], [3, 6, 9]]
{1 => 1, 2 => 1, 3 => 1}
10
{1 => [1, 3], 0 => [2, 4]}
2
[[1, 10], [2, 20], [3, 30], [4, 40]]
100
{}
nil
0
{}
[[1, 4], [2, 5], [3, nil]]
[2, 4, 6, 8, 10]
[1, 3, 5]
15
[1, 2, 3, 4, 5]
