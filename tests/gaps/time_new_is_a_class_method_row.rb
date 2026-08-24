# `Time.new` is a CLASS-METHOD row in zeo. In ruby it is `Class#new`, and
# `Time#initialize` is the constructor. Two things follow, and both diverge.
#
#   Time.method(:new).owner              ruby: Class   zeo: #<Class:Time>
#   Time.singleton_methods(false)        ruby: no :new   zeo: has :new
#
# And because construction never reaches `Class#new`, a program that reopens
# `Time` with its own `initialize` is ignored -- the row that would unfuse
# `allocate` plus `initialize` is never consulted. `Pathname` is the same
# shape and is FIXED (tests/a_reopened_rust_class_runs_a_ruby_initialize.rb),
# which is what narrows this to Time's constructor rather than the mechanism.
#
# The fix is to make `Time#initialize` the real constructor and delete the
# class-method row, which is ruby's own layout. What stops it being a small
# change: `RTime` is immutable apart from its rendering `offset`, so an
# `initialize` running on an ALLOCATED receiver has nowhere to write the
# instant. `num`/`den` would move behind the same mutex the offset uses.
#
# `Time.allocate` itself already matches, including the `uninitialized Time`
# refusal on every row that reads the instant -- see
# tests/an_allocated_time_is_uninitialized.rb.
p Time.method(:new).owner
p Time.singleton_methods(false).sort

class Time
  def initialize(*) = @t = "T"
  def mine = @t
end
p [Time.new.mine, Time.new.instance_variables]
