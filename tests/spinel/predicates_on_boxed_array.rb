# all?/any?/none?/one? on a BOXED array (one bound from a destructured
# multi-value return, then mapped): the blockless forms had no arm and raised
# NoMethodError naming Array, and the block form with an unused parameter did
# not compile (#3967).
def pass
  [[[0, "x"]], 2]
end
layout, size = pass
r = layout.map { |a| a }
p [r.all?, r.any?, r.none?, r.one?]
p r.all? { |x| true }
p r.any? { |x| x.size > 1 }
s = layout.select { |a| true }
p s.all?
e = layout.each_with_index.map { |a, i| a }
p e.all?
n = [nil, false].map { |x| x }
p [n.all?, n.any?, n.none?, n.one?]
m = [nil, 1].map { |x| x }
p [m.all?, m.any?, m.none?, m.one?]
p size
