# `Hash.new { ... }` lowers to its own `HirNode::New`, not to an ordinary
# call, and the block hangs off that node. Two walks that decide whether a
# method needs its `__blk` parameter used to match `New { args, .. }` and so
# never looked at the block or the keyword arguments at all -- a `yield` in a
# default-value block was invisible, and the method came out with no block
# parameter to yield to.
def with_default
  h = Hash.new { |_hash, key| yield key }
  [h[:a], h[:b]]
end

p(with_default { |k| "made:#{k}" })

# `Array.new`'s block is the same node shape.
def sized(n)
  Array.new(n) { |i| yield i }
end

p(sized(3) { |i| i * i })

# ...and so is a `Struct.new` body, where the yield sits one level deeper.
def build_point
  Struct.new(:x, :y) do
    define_method(:label) { "point" }
  end
end

Point = build_point
p Point.new(1, 2).label

# `block_given?` is the other half of the same question, and reaches the same
# walk through the same node.
def maybe_default
  h = Hash.new { |_hash, _key| block_given? }
  h[:anything]
end

p maybe_default
p(maybe_default { :unused })
