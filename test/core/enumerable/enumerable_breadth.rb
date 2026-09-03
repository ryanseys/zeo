# Enumerable chunking/slicing and Array combinatorics/search.
#
# Everything here is driven by the receiver's own `each`, so it works the
# same on an Array, a Range, or a user class that includes Enumerable --
# which the last section shows.

# --- chunk_while / slice_when ----------------------------------------------
# Exact negations of each other: both walk adjacent pairs and cut between
# them, `chunk_while` when the block is false, `slice_when` when it's true.
runs = [1, 2, 4, 9, 10, 11, 12, 15]
p runs.chunk_while { |i, j| i + 1 == j }.to_a
p runs.slice_when { |i, j| i + 1 != j }.to_a
p [1, 1, 2, 3, 3].chunk_while { |i, j| i == j }.to_a

# No adjacent pair means the block never runs.
p [].chunk_while { |i, j| true }.to_a
p [7].chunk_while { |i, j| true }.to_a

# --- slice_before / slice_after --------------------------------------------
# `before` puts the match at the START of the next slice, `after` at the END
# of the current one.
p [1, 2, 3, 4, 5].slice_before { |x| x.even? }.to_a
p [1, 2, 3, 4, 5].slice_after { |x| x.even? }.to_a

# A match at the very start opens the first slice -- it does not close an
# empty one.
p [1, 2, 3].slice_before { |x| x == 1 }.to_a

# Both also take a PATTERN instead of a block, matched with `===`.
p [1, 2, 3, 4, 5].slice_before(3).to_a
p [1, 2, 3, 4, 5].slice_after(3).to_a
p ["a", "b1", "c"].slice_before(/\d/).to_a

# --- grep / grep_v ---------------------------------------------------------
# Also `===`, so a Range covers, a Class checks is_a?, a Regexp matches.
p (1..10).grep(3..5)
p [1, "a", 2, "b"].grep(Integer)
p ["apple", "banana", "cherry"].grep(/an/)
p [1, "a", 2, "b"].grep_v(Integer)

# A block maps the survivors.
p [1, "a", 2, "b"].grep(Integer) { |x| x * 10 }
p [1, "a", 2, "b"].grep_v(Integer) { |x| x.upcase }

# --- zip -------------------------------------------------------------------
p [1, 2, 3].zip([4, 5, 6])
p [1, 2, 3].zip([4, 5], [6])      # short others pad with nil
p [1, 2].zip([3, 4], [5, 6])
p([1, 2].zip([3, 4]) { |pair| })  # the block form answers nil

# --- minmax_by -------------------------------------------------------------
p [1, 2, 3, 4].minmax_by { |x| -x }
p ["a", "bbb", "cc"].minmax_by { |s| s.length }
p [].minmax_by { |x| x }          # a nil PAIR, not []

# --- Array combinatorics ---------------------------------------------------
p [1, 2, 3].combination(2).to_a
p [1, 2, 3].combination(0).to_a   # one EMPTY tuple...
p [1, 2, 3].combination(4).to_a   # ...but none at all past the length
p [1, 2, 3].permutation(2).to_a
p [1, 2].permutation.to_a         # no argument = full length

# The block form yields each tuple.
picked = []
[1, 2, 3].combination(2) { |c| picked << c.sum }
p picked

# --- bsearch ---------------------------------------------------------------
# Find-minimum mode: the array is sorted so every false precedes every true,
# and the answer is the first true.
sorted = [1, 2, 3, 4, 5]
p sorted.bsearch { |x| x >= 3 }
p sorted.bsearch { |x| x >= 99 }  # nil when nothing matches
p sorted.bsearch_index { |x| x >= 3 }

# --- cycle -----------------------------------------------------------------
p [1, 2, 3].cycle(2).to_a
p [1, 2].cycle(0).to_a
seen = []
[1, 2].cycle(2) { |x| seen << x }
p seen

# With no count `cycle` repeats forever, so it only ends via `break`.
count = 0
[1, 2].cycle do |x|
  count += 1
  break if count == 5
end
p count

# --- the destructive forms -------------------------------------------------
# `flatten!` answers nil when nothing changed; `sort_by!` always answers the
# receiver.
p [[1, [2, 3]], [4]].flatten!
p [1, 2].flatten!
nested = [1, [2, [3, [4]]]]
p nested.flatten!(1)
ordered = [3, 1, 2]
ordered.sort_by! { |x| -x }
p ordered

# --- values_at / each_index ------------------------------------------------
p [1, 2, 3].values_at(0, 2, 5)     # out of bounds is nil, not skipped
p [1, 2, 3].values_at(-1, -3)
p [1, 2, 3].values_at(0..1)
p [1, 2, 3, 4, 5].values_at(3..9)  # one entry PER INDEX the range names
p [1, 2, 3].each_index.to_a

# --- Array.new -------------------------------------------------------------
p Array.new
p Array.new(3)
p Array.new(3) { |i| i * i }

# The default-value form SHARES one object across every slot -- which is
# exactly why the block form exists.
shared = Array.new(2, "x")
p shared
shared[0] << "!"
p shared

fresh = Array.new(2) { "x" }
fresh[0] << "!"
p fresh

# --- all of it works on a user Enumerable ----------------------------------
class Steps
  include Enumerable

  def initialize(*values)
    @values = values
  end

  def each(&blk)
    @values.each(&blk)
    self
  end
end

steps = Steps.new(1, 2, 4, 5, 6, 9)
p steps.chunk_while { |i, j| i + 1 == j }.to_a
p steps.grep(2..5)
p steps.minmax_by { |x| -x }
p steps.slice_before(&:even?).to_a
__END__
[[1, 2], [4], [9, 10, 11, 12], [15]]
[[1, 2], [4], [9, 10, 11, 12], [15]]
[[1, 1], [2], [3, 3]]
[]
[[7]]
[[1], [2, 3], [4, 5]]
[[1, 2], [3, 4], [5]]
[[1, 2, 3]]
[[1, 2], [3, 4, 5]]
[[1, 2, 3], [4, 5]]
[["a"], ["b1", "c"]]
[3, 4, 5]
[1, 2]
["banana"]
["a", "b"]
[10, 20]
["A", "B"]
[[1, 4], [2, 5], [3, 6]]
[[1, 4, 6], [2, 5, nil], [3, nil, nil]]
[[1, 3, 5], [2, 4, 6]]
nil
[4, 1]
["a", "bbb"]
[nil, nil]
[[1, 2], [1, 3], [2, 3]]
[[]]
[]
[[1, 2], [1, 3], [2, 1], [2, 3], [3, 1], [3, 2]]
[[1, 2], [2, 1]]
[3, 4, 5]
3
nil
2
[1, 2, 3, 1, 2, 3]
[]
[1, 2, 1, 2]
5
[1, 2, 3, 4]
nil
[1, 2, [3, [4]]]
[3, 2, 1]
[1, 3, nil]
[3, 1]
[1, 2]
[4, 5, nil, nil, nil, nil, nil]
[0, 1, 2]
[]
[nil, nil, nil]
[0, 1, 4]
["x", "x"]
["x!", "x!"]
["x!", "x"]
[[1, 2], [4, 5, 6], [9]]
[2, 4, 5]
[9, 1]
[[1], [2], [4, 5], [6, 9]]
