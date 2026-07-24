def dbl(n) = n * 2
p method(:dbl) === 5
p method(:dbl).super_method

class Base
  def greet = "base"
end
class Sub < Base
  def greet = "sub"
end
sm = Sub.new.method(:greet).super_method
p sm.class
p sm.call
p sm.inspect.start_with?("#<Method: Base#greet")
p Sub.instance_method(:greet).super_method.inspect.start_with?("#<UnboundMethod: Base#greet")
p Base.instance_method(:greet).source_location.is_a?(Array)
