# An absent Object method on a blank-slate receiver raises INSIDE the
# enclosing handler's region (#2726): the raise is an expression-arm prelude,
# which used to emit before the rescue modifier's setjmp.
class BO < BasicObject
  def initialize; @x = 1; end
  def own; @x; end
end
a = BO.new
r = (a.dup.own rescue $!.class)
p r
p a.own
