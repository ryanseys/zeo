# f0a93f3d types a poly receiver's `to_s` for what BOTH arms can hold, which is
# sp_RbVal when a user arm's return is untyped. Where the result lands in a
# String slot the assignment needs the unboxing coercion; #4090 reported it
# emitted uncoerced. It answers correctly now -- this pins the shape so the
# next arm added to that section has to keep it.

class Cand
  attr_accessor :id

  def initialize(id)
    self.id = id
  end

  def to_s
    id
  end
end

class Req
  attr_accessor :verb
end

def build(v)
  r = Req.new
  r.verb = (v || "GET").to_s
  r
end

p build(nil).verb
p build("POST").verb
p Cand.new("z").to_s

# the same through a local rather than a writer
def label(v)
  s = (v || "none").to_s
  s
end
p label(nil)
p label("x")
p label(Cand.new("c"))

# and into an interpolation, where the slot is a string too
def show(v) = "[#{(v || 'd').to_s}]"
p show(nil)
p show("e")
