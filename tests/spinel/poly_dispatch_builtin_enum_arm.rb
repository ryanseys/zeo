# A builtin Array receiver at a dispatch that exists only because a user class
# defines the same name.
#
# A block-carrying call on a builtin is normally served by splicing the block
# inline. That is not available once a user candidate takes the block as a real
# proc: the proc is materialized once and shared by every arm, and a second
# spliced copy would disagree with whichever arm ran. So the builtin had no arm
# at all and fell through to the raise -- NoMethodError on an Array, at a site
# where the call is plainly Array#map.
#
# The mirror of it is just as wrong and silent: the element-loop emitters walk
# a poly receiver as a container without asking whether a user class owns the
# name, so the user object iterated as an empty array and the call answered
# nothing. Only the cls_id dispatch can serve both, so those emitters decline
# to it and it grew a builtin arm that drives the same materialized proc.
#
# Reachable at all only since a yielding method became dispatchable.

class Rel
  def map
    out = []
    [1, 2].each { |x| out << yield(x) }
    out
  end

  def select
    out = []
    [1, 2, 3].each { |x| out << x if yield(x) }
    out
  end

  def reject
    out = []
    [1, 2, 3].each { |x| out << x unless yield(x) }
    out
  end

  def group_by
    h = {}
    [1, 2, 3, 4].each do |x|
      k = yield(x)
      h[k] = [] unless h.key?(k)
      h[k] << x
    end
    h
  end

  def each
    yield 7
    yield 8
    self
  end

  def find
    yield 1
    :rel_find
  end

  def find_index
    yield 1
    -7
  end

  def min_by
    yield 1
    :rel_min
  end

  def max_by
    yield 1
    :rel_max
  end

  def partition
    yield 1
    [[:a], [:b]]
  end

  def sum
    yield 1
    -8
  end

  def any?
    yield 1
    false
  end

  def all?
    yield 1
    false
  end

  def none?
    yield 1
    false
  end

  def take_while
    yield 1
    [:tw]
  end

  def drop_while
    yield 1
    [:dw]
  end

  def flat_map
    yield 1
    [:fm]
  end

  def each_with_index
    yield 1
    [:ewi]
  end

  # a non-yielding, blockless method of an Enumerable name: the dispatch still
  # exists, and the builtin arm has to materialize the block itself because no
  # candidate asked for one
  def sort_by
    [9]
  end

  def count
    -1
  end
end

def pick(n)
  n == 1 ? Rel.new : [3, 4, 5, 6]
end

p pick(1).map { |x| x * 2 }
p pick(2).map { |x| x * 2 }

p pick(1).select { |x| x > 1 }
p pick(2).select { |x| x > 4 }

p pick(1).reject { |x| x > 1 }
p pick(2).reject { |x| x > 4 }

p pick(1).group_by { |x| x % 2 }
p pick(2).group_by { |x| x % 2 }

seen = []
pick(1).each { |x| seen << x }
pick(2).each { |x| seen << x }
p seen

# Array#each answers the receiver
p pick(2).each { |x| x }

p pick(1).find { |x| x > 0 }
p pick(2).find { |x| x > 4 }
p pick(1).find_index { |x| x > 0 }
p pick(2).find_index { |x| x > 4 }
p pick(1).min_by { |x| -x }
p pick(2).min_by { |x| -x }
p pick(1).max_by { |x| -x }
p pick(2).max_by { |x| -x }
p pick(1).partition { |x| x.odd? }
p pick(2).partition { |x| x.odd? }
p pick(1).sum { |x| x }
p pick(2).sum { |x| x * 10 }
p pick(1).any? { |x| x > 0 }
p pick(2).any? { |x| x > 5 }
p pick(2).all? { |x| x > 5 }
p pick(2).none? { |x| x > 9 }
p pick(1).take_while { |x| true }
p pick(2).take_while { |x| x < 5 }
p pick(2).drop_while { |x| x < 5 }
p pick(1).flat_map { |x| [x] }
p pick(2).flat_map { |x| [x, x] }
p pick(1).each_with_index { |x, i| x }
p pick(2).each_with_index { |x, i| x }

# no candidate asked for the block, so the builtin arm materializes it
p pick(1).sort_by { |x| -x }
p pick(2).sort_by { |x| -x }
p pick(1).count { |x| x.even? }
p pick(2).count { |x| x.even? }

# a captured local through the builtin arm: the proc is the one the user arm
# would have taken, so its capture cell has to work here too
base = 100
p pick(2).map { |x| base + x }

# strings, so the element rides the pointer channel
def pick_s(n)
  n == 1 ? Rel.new : ["a", "bb", "ccc"]
end

p pick_s(2).map { |s| s.length }
p pick_s(2).sort_by { |s| -s.length }
p pick_s(2).group_by { |s| s.length.odd? }

# a Hash receiver: entries arrive as [k, v] pairs, and a two-parameter block
# autosplats them the way a spliced loop would
def pick_h(n)
  n == 1 ? Rel.new : { "a" => 1, "b" => 2 }
end

p pick_h(2).map { |k, v| "#{k}#{v}" }
p pick_h(2).select { |k, v| v > 1 }
acc = []
pick_h(2).each { |k, v| acc << k }
p acc
