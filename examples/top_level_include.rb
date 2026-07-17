# A top-level `include` mixes a module into Object (the main object's class),
# so its methods and constants become available bare, program-wide.

# Math's module functions and constants after `include Math`.
include Math
p sqrt(16)
p cos(0)
p log(E).round(4)
p PI.round(4)
p hypot(3, 4)

# A user module's instance methods mix in the same way.
module Greeter
  GREETING = "hello"
  def greet(who) = "#{GREETING}, #{who}"
end
include Greeter
p greet("world")
p GREETING

# The mixed-in methods are visible from inside other objects too (Object is
# everyone's ancestor).
class Widget
  def describe = "area #{sqrt(144).to_i}, #{greet('widget')}"
end
p Widget.new.describe
