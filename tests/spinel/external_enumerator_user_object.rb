# An external enumerator over a user object. sp_enum_items_from -- which is
# what materializes an enumerator's elements -- had arms for every builtin
# container and none for a user object, so it answered the EMPTY array and
# `v.each.take(3)` yielded nothing (#4022).
#
# The report needed a `&:sym` map over an Array of the same class to see it:
# that is what makes the object arrive boxed here rather than through a
# spliced loop.
class Box
  include Enumerable

  attr_reader :size

  def initialize(items)
    @items = items
    @size = items.size
  end

  def each
    return to_enum(:each) unless block_given?

    @items.each { |i| yield i }
    self
  end
end

boxes = [Box.new([0, 1]), Box.new([0, 1, 4, 9])]
p boxes.map(&:size)
v = boxes.last
p v.each.take(3)
p v.each.to_a
p v.each.first(2)
p v.to_a
p v.map { |x| x * 2 }

# and the same object without the map line ahead of it
w = Box.new([7, 8, 9])
p w.each.take(2)
