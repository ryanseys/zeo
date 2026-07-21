# A yield-inline wrapper called from sites whose blocks return DIFFERENT types:
# the collector the inner iterator builds must hold poly elements. One shared
# inlined body cannot size it for both an Integer- and a String-returning block
# -- whichever site is analyzed first would settle the slot and the other would
# emit into it (#2457, same family as the `acc << yield(x)` case in #2454).
class Row
  def initialize(i, c); @i = i; @c = c; end
  def id; @i; end
  def category; @c; end
end
class Relation
  def initialize(rows); @rows = rows; end
  def to_a; @rows; end
  def map; to_a.map { |x| yield x }; end
end
r = Relation.new([Row.new(1, "news"), Row.new(2, "ask")])
puts r.map { |x| x.category }.join(" ")
p r.map { |x| x.id }
p r.map { |x| x.id }.length
p r.map { |x| x.id.to_f }

# a wrapper whose sites all agree keeps its concrete element type
class IntsOnly; def map; [1, 2].map { |x| yield x }; end; end
i = IntsOnly.new
p i.map { |x| x + 1 }
p i.map { |x| x * 2 }
