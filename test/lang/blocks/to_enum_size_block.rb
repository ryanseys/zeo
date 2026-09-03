# `to_enum`/`enum_for` take a block that supplies the enumerator's `size`
# lazily. zeo drops it, so a custom Enumerable's `size` answers nil.
#
#     def each(&b) = block_given? ? (@items.each(&b); self) : to_enum(:each) { @items.size }
#
# That line is the documented way to write `each`, and the block exists so the
# count is computed only if someone asks -- which is what makes `size` usable on
# an enumerator over something expensive to walk. Anything that reads it to
# preallocate, to drive a progress bar, or to decide a strategy gets nil and has
# to fall back to walking.
#
# zeo's Enumerator already carries a size (`each_slice` reports one below), so
# the field exists; `to_enum` just does not accept the block that fills it.

class Bag
  include Enumerable
  def initialize(*items) = @items = items
  def each(&b) = block_given? ? (@items.each(&b); self) : to_enum(:each) { @items.size }
end

e = Bag.new(1, 2, 3).each
p e.size
p e.to_a

class Counter
  def go = enum_for(:go) { 7 }
end
p Counter.new.go.size

class Pairs
  def pairs(n)
    return to_enum(:pairs, n) { n } unless block_given?
    n.times { |i| yield i, i * 2 }
  end
end
pe = Pairs.new.pairs(3)
p pe.size
p pe.to_a

# A builtin enumerator already reports its size.
p [1, 2].each_slice(1).size
p (1..10).each_entry.size
__END__
3
[1, 2, 3]
7
3
[[0, 0], [1, 2], [2, 4]]
2
10
