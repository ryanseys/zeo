# find/detect take an optional ifnone callable, invoked (no args) only when
# nothing matches; a match -- even a nil element -- ignores it.
p [1, 2, 3].find(-> { -1 }) { |x| x > 10 }
p [1, 20, 3].find(-> { -1 }) { |x| x > 10 }
p [1, nil, 3].find(-> { :fallback }) { |x| x.nil? }

# tally accepts an accumulator hash: counts add onto its existing values and
# the same hash is returned.
h = Hash.new(0)
p [1, 1, 2, 3, 3, 3].tally(h)
p h
p [5, 5, 6].tally({ 5 => 10 })
p [1, 1, 2].tally

# min_by(n)/max_by(n) return the n smallest/largest by key, ascending for
# min_by and descending for max_by; no count answers a single element.
p %w[bbbb a ccc dd].max_by(2, &:length)
p %w[bbbb a ccc dd].min_by(2, &:length)
p [3, 1, 4, 1, 5, 9, 2].max_by(3) { |n| n }
p [1, 2, 3].min_by(0) { |n| n }
p %w[bb a ccc dd].max_by(&:length)

# Proc#source_location answers a [String, Integer] pair.
loc = ->(x) { x }.source_location
p loc.class
p loc.length
p [loc[0].class, loc[1].class]
__END__
-1
20
nil
{1 => 2, 2 => 1, 3 => 3}
{1 => 2, 2 => 1, 3 => 3}
{5 => 12, 6 => 1}
{1 => 2, 2 => 1}
["bbbb", "ccc"]
["a", "dd"]
[9, 5, 4]
[]
"ccc"
Array
2
[String, Integer]
