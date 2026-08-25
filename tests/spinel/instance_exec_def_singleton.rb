# A `def` inside instance_exec defines a singleton method on the receiver
# at runtime (CRuby), and zeo matches.
class Box
  def initialize(v)
    @v = v
  end
end

b = Box.new(5)
b.instance_exec { def greet; "hi"; end }
puts b.greet
