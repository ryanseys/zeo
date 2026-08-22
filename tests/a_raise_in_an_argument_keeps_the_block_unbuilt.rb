# A literal block's proc is MOVED to the callee, so no landing can release
# it: on every path but the raise the callee has already taken the value
# out of the slot. That makes the window between building the proc and
# making the call load-bearing -- an argument that raises inside it leaks
# the proc, once per execution.
#
# The emitter used to build the proc FIRST, before the receiver and the
# argument list, at five call shapes. Each of the sends below raises from
# an argument, and each leaked one `Proc` (tag 24) per iteration. The block
# is now built last, which is also ruby's own order.
#
# `ZEO_RT_LEAKCHECK=1` is what makes this a test: the answers printed here
# are the same either way, and the ledger's exit balance is the measurement.

def missing
  raise ArgumentError, "no argument for you"
end

def takes_one(n)
  yield n
end

class Holder
  def initialize(n)
    @n = n
  end
end

def attempt
  yield
  "no raise"
rescue ArgumentError
  "raised"
end

# A call to a compiled method, whose block rides its own parameter.
p attempt { takes_one(missing) { |x| x } }

# A dynamic send on a receiver the compiler cannot name.
p attempt { [1, 2].fetch(missing) { |x| x } }

# A splatted argument list, which builds its Array in the runtime.
p attempt { takes_one(*[missing]) { |x| x } }

# A keyword send.
p attempt { [1, 2].each_slice(missing) { |x| x } }

# A constructor, whose block forwards to `initialize`.
p attempt { Holder.new(missing) { |x| x } }

# Once more in a loop: a leak here is unbounded, not a one-off.
10.times { attempt { takes_one(missing) { |x| x } } }
puts "done"
