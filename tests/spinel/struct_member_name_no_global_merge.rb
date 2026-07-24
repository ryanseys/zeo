# A Struct member name must not globally type-merge with an unrelated
# method of the same name. Here Struct member `b` (an Integer) shares
# its name with Weight#b (a Float array); the int use through an
# unresolved receiver must stay Integer, not get widened to FloatArray.
class Weight
  def initialize
    @v = [1.0, 2.0, 3.0]
  end
  def b
    @v
  end
end
Ctx = Struct.new(:t, :b)
def sum_to(batch)
  r = batch % 7
  i = 0
  s = 0
  while i < batch
    s = s + i
    i = i + 1
  end
  s + r
end
w = Weight.new
puts w.b.length
ctx = Ctx.new(3, 5)
puts sum_to(ctx.b)
puts sum_to(10)
