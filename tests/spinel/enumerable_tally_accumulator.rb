# Enumerable#tally(hash) counts into the given accumulator hash and returns it
# (the same object), across hash variants (#2628).
class E
  include Enumerable
  def initialize(*a) = @a = a
  def each(&b) = @a.each(&b)
end
h = {1 => 10}
r = E.new(1, 2, 2).tally(h)
p r
p r.equal?(h)
p(E.new("a", "b", "a").tally({"a" => 5}))
p([1, 1, 2].tally({2 => 100}))
