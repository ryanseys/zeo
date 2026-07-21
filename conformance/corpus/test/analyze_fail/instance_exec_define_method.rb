# define_method inside instance_exec is a runtime method definition
# Spinel does not support; reject at compile time. (CRuby raises
# NoMethodError -- define_method is private -- but the point is the same:
# no static lowering exists.)
class Box
  def initialize(v)
    @v = v
  end
end

class BoxPlus < Box
end

b = BoxPlus.new(5)
b.instance_exec { define_method(:greet) { 1 } }
