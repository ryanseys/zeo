# `super`, `yield` and `return` need the enclosing method's identity,
# block channel and return target -- which a snippet's own level does
# not have, but a `def` written INSIDE it does: the emitter's
# run-time-installed body reads its defining class off the method
# frame stack. So the refusal is about where they sit, not what they
# are.

class Base
  def greet = "base"
end
class Kid < Base; end
src = "def greet; %(kid+) + super; end"
Kid.class_eval(src)
p Kid.new.greet
yielder = "def y_it; yield 5; end"
eval(yielder)
p(y_it { |v| v * 3 })
__END__
"kid+base"
15
