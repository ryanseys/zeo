# The blockless index enumerators on a poly receiver built an Enumerator over
# the object without asking whether a user class owns the name: the method was
# never entered, its side effects were dropped, and iterating the result
# yielded nothing. The typing rule above it deliberately answers Enumerator
# because a boxed receiver can always be an Array (#4021) -- true, and the
# reason the user arm's own answer has nowhere to go unless the call is poly.
class Rows
  def initialize
    @seen = []
  end

  attr_reader :seen

  def each_index
    @seen.push(:ran)
    [7]
  end
end

def indices(v) = v.each_index

r = Rows.new
got = indices(r)
p r.seen
p got.to_a
got.each { |x| p x }

p indices([1, 2]).to_a
p indices(["a", "b", "c"]).to_a

class Pairs
  def each_with_index
    ["p"]
  end
end

def pairs(v) = v.each_with_index
p pairs(Pairs.new).to_a
p pairs([1, 2]).to_a

# with no user class in sight the enumerators are what they were
p [1, 2, 3].each_index.to_a
p [1, 2].each_with_index.to_a
