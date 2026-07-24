# map over an array of objects where the block returns a polymorphic
# field (sp_RbVal) must collect into a PolyArray, not a PtrArray that
# casts the sp_RbVal element through (void *).
class Box
  def initialize
    @v = 0
  end

  def taint
    @v = "x"
  end

  def v
    @v
  end
end

boxes = []
boxes.push(Box.new)
b2 = Box.new
b2.taint
boxes.push(b2)

vs = boxes.map { |b| b.v }
puts vs.length
p vs
p vs[0]
p vs[1]
