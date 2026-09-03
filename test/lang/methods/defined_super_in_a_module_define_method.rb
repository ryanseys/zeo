# `defined?(super)` in a scope with no method NAME of its own, and the bare
# `super` beside it.
#
# Such a scope is either outside any method -- the top level, a class body,
# where ruby answers nil -- or a block that BECOMES a method at run time
# (`K.define_method(:m) { }`, and the same call through `send`, which the
# compiler cannot recognize as a definition at all). Only the run time can
# tell those apart, and it does: the same method-frame stack
# `send_super_dynamic` resumes from, empty outside a method.
#
# The static classification answered "method" for every one of them, so a body
# with no super target at all reported one -- and `defined?(super) ? super :
# fallback`, the idiom a mixin that may or may not have a target is written
# with, took the `super` branch and raised.
#
# The BARE `super` is the other half. A zsuper forwards the method's
# parameters and a block-shaped body has none, so ruby refuses it by name; a
# scope that never became a method gets `NoMethodError` instead. The compiler
# knows which only for the `define_method` spelling it can see, so the run
# time picks.

class Base
  def m = "base"
end
class Sub < Base
  define_method(:m) { "sub(" + (defined?(super) ? "has" : "none") + ")" }
end
p Sub.new.m

class Lone
  define_method(:n) { defined?(super) ? "has" : "none" }
end
p Lone.new.n

module Mx
end
class Host
  include Mx
end
Mx.send(:define_method, :q) { defined?(super) ? "has" : "none" }
p Host.new.q

# A scope that never becomes a method, in each of its spellings.
def outside
  [
    (begin; super; rescue => e; [e.class.to_s, e.message]; end),
    (begin; super(); rescue => e; [e.class.to_s, e.message]; end),
    defined?(super).inspect,
  ]
end
p outside
[1].each { p [(begin; super; rescue => e; e.class.to_s; end), defined?(super).inspect] }

# A block inside a REAL method still means that method's super.
class Base2
  def m = "base2"
end
class Sub2 < Base2
  def m = [1].map { defined?(super) ? super : "none" }
end
p Sub2.new.m

# An explicit-argument super through the `send` spelling reaches the target.
class Sub3 < Base2
end
Sub3.send(:define_method, :m) { "s3(" + super() + ")" }
p Sub3.new.m

# A bare one is ruby's own refusal, by either spelling.
class Sub4 < Base2
end
Sub4.send(:define_method, :m) { "s4(" + super + ")" }
begin
  Sub4.new.m
rescue => e
  p [e.class.to_s, e.message]
end
__END__
"sub(has)"
"none"
"none"
[["NoMethodError", "super: no superclass method 'outside' for main"], ["NoMethodError", "super: no superclass method 'outside' for main"], "nil"]
["NoMethodError", "nil"]
["base2"]
"s3(base2)"
["RuntimeError", "implicit argument passing of super from method defined by define_method() is not supported. Specify all arguments explicitly."]
