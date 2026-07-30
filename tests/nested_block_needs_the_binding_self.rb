# A block nested inside another block, in a program that anywhere asks a Proc
# for its `binding`. The inner Proc's construction carries a Binding of the
# scope it is WRITTEN in -- the outer block -- and that Binding holds the
# outer block's `self`. Zeo built the Binding in the outer block's own
# prelude, INSIDE its `move` closure, while the outer block took no `self`
# parameter of its own because nothing it does mentions `self`. So the
# closure swallowed the method's receiver whole: an `Fn` closure cannot
# consume a captured value, and the method lost it for good.
#
# irb is written this way throughout -- `irb.suspend_name(path) { |io|
# irb.suspend_input_method(io) { |back_io| ... } }` in ext/loader.rb -- and it
# accounted for 41 of the 51 rustc errors the vendored gem produced.
class Wrapper
  def wrap
    yield "w"
  end
end

class Host
  def initialize
    @tag = "t"
  end

  def run(path)
    w = Wrapper.new
    acc = []
    w.wrap do |a|
      acc << path
      w.wrap do |b|
        acc << (a + b)
      end
    end
    [acc, @tag]
  end

  # The same nesting where the inner block is a lambda, and where the outer
  # one really does reach for the Binding.
  def peek
    w = Wrapper.new
    seen = nil
    w.wrap do |a|
      inner = -> { a.upcase }
      seen = binding.local_variables.sort
      inner.call
    end
    seen
  end
end

h = Host.new
p h.run("a")
p h.peek

# What made the whole scope a binding scope in the first place.
p proc { 1 }.binding.class
p proc { 1 }.binding.receiver.class

# The receiver a nested block's Binding reports is the enclosing `self`, not
# the block's own -- the value that used to be moved away.
class Reporter
  def call
    outer = nil
    [1].each do |x|
      outer = proc { x }.binding.receiver
    end
    outer
  end
end

p Reporter.new.call.class
