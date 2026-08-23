# A `Data` instance freezes inside `Data#initialize`, which is where CRuby
# freezes it (`rb_data_initialize_m`) -- not after the constructor returns.
# The difference is visible from a subclass:
#
#   * one that writes an ivar AFTER `super` must raise `FrozenError`;
#   * one that never calls `super` leaves the instance MUTABLE, because
#     nothing froze it.
#
# zeo froze after dispatching `initialize`, so the first shape silently
# succeeded and the second was frozen when ruby leaves it open.
#
# The message that raise carries needed one more fix. `check_frozen` renders
# the receiver through the runtime's own infallible inspect, which had no arm
# for a struct or data instance and fell through to the address form -- so the
# message read `#<D2:0x...>` where CRuby says `#<data D2 m=1>`. The Ruby-level
# `inspect` was already right; it is a table row, which `call_user_method`
# cannot see.

D = Data.define(:m)

class AfterSuper < D
  def initialize(**kw)
    super
    @z = 1
  end
end
begin
  AfterSuper.new(m: 1)
rescue => e
  p e.class
  p e.message
end

class NoSuper < D
  def initialize(**kw)
    @z = 1
  end
end
o = NoSuper.new(m: 1)
p [o.frozen?, o.instance_variables, o.m]

class BeforeSuper < D
  def initialize(**kw)
    @z = 1
    super
  end
end
b = BeforeSuper.new(m: 2)
p [b.m, b.instance_variables, b.frozen?]

# The plain form is frozen, as it always was.
p D.new(m: 3).frozen?
p D.new(m: 3).inspect

# A Struct is NOT frozen by its constructor, which is the contrast Data draws.
S = Struct.new(:a)
class SafterSuper < S
  def initialize(*args)
    super
    @z = 1
  end
end
s = SafterSuper.new(1)
p [s.a, s.instance_variables, s.frozen?]
p S.new(1).inspect
