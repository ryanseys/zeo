# DECIDED DIVERGENCE, and only on the GENERATOR half now: emitting a
# structure nested past 2,000 deep raises `JSON::NestingError` where ruby
# recurses until the machine stack ends.
#
# PARSING used to be bounded the same way and no longer is. The parser keeps
# its open containers on an explicit stack, so nesting costs heap and
# `max_nesting: false` means what it says -- the same guarantee ruby's own
# parser gives. That change also removed the reason the old limit existed:
# the 2,000 was MEASURED against a fiber's stack, and a measured number goes
# stale the moment the parse runs somewhere with less stack than the bench
# had. A bound that has to be re-measured per environment is not a bound.
#
# What is left is two rows where the two engines answer differently, and zeo
# is the safer one in both:
#
#   far past it       100,000 levels. Ruby's parser raises SystemStackError;
#                     zeo answers the Array, because nothing about its depth
#                     touches the machine stack.
#   generate past it  Ruby emits 4,202 bytes here, and on a CYCLIC structure
#                     the same code raises SystemStackError -- it has no
#                     bound but the stack. Zeo's generator still recurses, so
#                     its depth counter is what stands between a cycle and a
#                     dead process. A loud error the program can rescue beats
#                     an abort with no line of output, every time.
#
# The generator's own default of 100 still applies when nobody turns it off,
# and ruby says so too: `nesting of 100 is too deep. Did you try to serialize
# objects with circular references?`

require "json"

def show(name)
  r = yield
  puts "#{name}\t#{r.inspect}"
rescue Exception => e
  puts "#{name}\t#{e.class}"
end

deep = ->(n) { "[" * n + "1" + "]" * n }

show("at the ceiling") { JSON.parse(deep.call(2_000), max_nesting: false).class }
show("past it") { JSON.parse(deep.call(2_001), max_nesting: false).class }
show("far past it") { JSON.parse(deep.call(100_000), max_nesting: false).class }

# `parse!` turns the limit off too, and is unbounded the same way.
show("parse! past it") { JSON.parse!(deep.call(5_000)).class }

# In a THREAD, whose stack is the runtime's own 8 MiB.
show("in a thread") do
  Thread.new do
    JSON.parse(deep.call(2_001), max_nesting: false).class
  rescue Exception => e
    e.class
  end.value
end

# In a FIBER, whose stack is the smallest of the three -- the case that broke
# the old measured ceiling.
show("in a fiber") do
  f = Fiber.new do
    Fiber.yield(begin
      JSON.parse(deep.call(2_001), max_nesting: false).class
    rescue Exception => e
      e.class
    end)
  end
  f.resume
end

# GENERATING is still bounded, because the depth counter is also its cycle
# guard.
show("generate past it") do
  a = []
  c = a
  2_100.times do
    n = []
    c << n
    c = n
  end
  JSON.generate(a, max_nesting: false).size
end

# A cycle is what that bound is FOR. Ruby answers SystemStackError here.
show("a cycle with the limit off") do
  a = []
  a << a
  JSON.generate(a, max_nesting: false).size
end

# ...and everything under the generator's ceiling still works.
show("just under") { JSON.parse(deep.call(1_999), max_nesting: false).flatten.first }
show("the gem's own default still applies") { JSON.parse(deep.call(101)) }
