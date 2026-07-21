# A `def` inside instance_exec defines a singleton method at runtime,
# which Spinel does not model -- it must be a compile-time error, not a
# silent miscompile. (CRuby defines a singleton method on the receiver.)
class Box
  def initialize(v)
    @v = v
  end
end

class BoxPlus < Box
end

b = BoxPlus.new(5)
b.instance_exec { def greet; end }
