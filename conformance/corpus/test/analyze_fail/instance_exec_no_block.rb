# instance_exec with no block is a LocalJumpError in CRuby; surface it
# at compile time rather than emitting a call to a block that isn't there.
class Box
  def initialize(v)
    @v = v
  end
end

class BoxPlus < Box
end

b = BoxPlus.new(5)
b.instance_exec
