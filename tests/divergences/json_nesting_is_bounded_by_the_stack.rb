# DECIDED DIVERGENCE. `max_nesting: false` does not mean unbounded here: a
# document nested past 2,000 deep raises `JSON::NestingError` where ruby
# keeps parsing.
#
# Ruby's parser keeps its own explicit stack and reads a MILLION-deep
# document without complaint. Zeo's is a recursive descent, so the machine
# stack is the real limit -- and past it the process ends with no exception
# to catch and no line of output. A loud error the program can rescue beats
# that, every time.
#
# The number is MEASURED on the SMALLEST stack a parse can run on, not on
# the main thread's. A `Fiber` was the case that mattered: 4,000 levels ran
# and 8,000 ended the process without a word. 2,000 sits below the smaller
# of those with room for whatever the program had on the stack already, and
# it is still twenty times the gem's OWN default of 100.
#
# The rows below are what the guarantee means: the limit holds wherever the
# parse runs, and generating is bounded the same way.

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

# `parse!` turns the limit off too, and is bounded the same way.
show("parse! past it") { JSON.parse!(deep.call(5_000)).class }

# In a THREAD, whose stack is the runtime's own 8 MiB.
show("in a thread") do
  Thread.new do
    JSON.parse(deep.call(2_001), max_nesting: false).class
  rescue Exception => e
    e.class
  end.value
end

# In a FIBER, whose stack is the smallest of the three -- the case the
# ceiling is measured against.
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

# GENERATING is bounded the same way, for the same reason.
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

# ...and everything at or under the ceiling still works.
show("just under") { JSON.parse(deep.call(1_999), max_nesting: false).flatten.first }
show("the gem's own default still applies") { JSON.parse(deep.call(101)) }
