# Enumerable (via a user class with #each) versions of grep/grep_v/sum/each_entry,
# the receiver KieranP's #2479/#2480/#2504/#2516 duplicates exercise. The bug was
# the __enum_to_a lowering picking the wrong element/result type.
class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
p(Nums.new("apple", "banana", "cherry").grep(/an/))
p(Nums.new("apple", "banana", "cherry").grep_v(/an/))
p(Nums.new("a", "b", "c").sum(""))
p(Nums.new(1.5, 2.5, 3.0).sum)
p(Nums.new(1.5, 2.5, 3.0).sum { |x| x * 2 })
a = Nums.new(3, 1, 2)
r = a.each_entry { |x| }
p r.equal?(a)
