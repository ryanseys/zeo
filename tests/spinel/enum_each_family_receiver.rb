# Enumerable/Struct each-family block calls (reverse_each, each_with_index)
# return the receiver, not the intermediate member array, and the synthesized
# __enum_to_a bridge stays invisible to reflection while user methods do not.
class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
  def __private_helper; 42; end
end
n = Nums.new(3, 1, 2)
acc = []
r = n.reverse_each { |x| acc << x }
p acc
p r.equal?(n)
p n.each_with_index { |x, i| }.equal?(n)
p n.respond_to?(:__enum_to_a)
p n.respond_to?(:__private_helper)
p n.respond_to?(:map)
p Nums.instance_methods(false).include?(:__enum_to_a)
p Nums.instance_methods(false).include?(:__private_helper)

Pt = Struct.new(:a, :b, :c)
s = Pt.new(3, 1, 2)
sb = []
sr = s.reverse_each { |v| sb << v }
p sb
p sr.equal?(s)
p s.respond_to?(:__enum_to_a)
