# DECIDED DIVERGENCE, now down to ONE row, and it is a cycle.
#
# BOTH halves are iterative. The parser and the generator each keep their
# open containers on an explicit stack, so nesting costs heap and
# `max_nesting: false` means what it says -- the same guarantee ruby's own
# parser gives, and better than what ruby's generator gives.
#
# `generate past it` used to be a divergence and is not any more: zeo emits
# the same 4,202 bytes ruby does. The generator had a hard 2,000-deep
# ceiling because it recursed, and a depth count cannot tell a document that
# is legitimately DEEP from one that is CIRCULAR -- so it refused both. They
# are told apart now: depth costs heap, and a cycle is caught by ANCESTRY,
# a container appearing inside itself.
#
# What is left is two rows where the engines still answer differently, and
# zeo is the safer one in both:
#
#   far past it       100,000 levels. Ruby's parser raises SystemStackError;
#                     zeo answers the Array, because nothing about its depth
#                     touches the machine stack.
#   a cycle with      Ruby raises SystemStackError -- its generator has no
#   the limit off     bound but the stack. Zeo raises JSON::NestingError,
#                     naming the circular reference, because it found one
#                     rather than ran out of room. A loud error the program
#                     can rescue beats an abort with no line of output.
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

# GENERATING past the old ceiling now emits, as ruby does.
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
