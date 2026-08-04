# WAITING ON `builtin_frames_in_a_backtrace`. Complex now holds a user
# `Numeric` subclass and dispatches every component operation, so this file
# raises ruby's ArgumentError from ruby's line with ruby's message -- what is
# left is the three C frames under it. zeo's native rows push no frame, so
# `Comparable#<`, `Complex#inspect` and `Kernel#p` are all missing from the
# trace. Promote this the moment that gap closes; nothing here needs Complex
# work any more.
#
# The original cause: `Numeric#i` builds `Complex(0, self)`, and zeo's Complex
# held only its own numeric lanes, so a user subclass got a TypeError where
# ruby builds `(0+Deg(7)*i)`.
#
# This file used to record a second, unrelated limit -- that nothing called
# `Numeric#singleton_method_added`, so `def n.foo` on a number succeeded where
# ruby refuses. The definition hooks now fire; see
# `tests/definition_hooks_at_runtime.rb`.

class Deg < Numeric
  def initialize(v) = @v = v
  def to_f = @v.to_f
  def inspect = "Deg"
end

n = Deg.new(7)

begin
  p n.i
rescue TypeError => e
  puts "i: #{e.message}"
end
