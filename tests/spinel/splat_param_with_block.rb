# A splat parameter collects the call's arguments even when the call site
# also passes a block: the block routes the call through the inlining path,
# which used to bind parameters positionally and drop the rest packing --
# leaving the first argument in the rest slot as a pointer of the wrong type.
class Thing; end

class Utils
  def self.silence_stream(*streams)
    p streams.length
    p streams.first.class
    yield
  end
  def self.with_pre(first, *rest)
    p first
    p rest
    yield
  end
  def self.with_post(*mid, last)
    p mid
    p last
    yield
  end
  def self.with_kw(*args, mode: "x")
    p args
    p mode
    yield
  end
  def inst(*a)
    p a
    yield
  end
end

p Utils.silence_stream(Thing.new) { 1 }
p Utils.silence_stream(1, 2, 3) { 2 }
p Utils.silence_stream { 3 }
p Utils.with_pre(1, 2, 3) { 4 }
p Utils.with_post(1, 2, 3) { 5 }
p Utils.with_kw(1, 2, mode: "y") { 6 }
p Utils.new.inst("a", "b") { 7 }
arr = [4, 5]
p Utils.silence_stream(*arr) { 8 }
class Box
  def initialize(*items)
    @items = items
    yield items.length if block_given?
  end
  def items = @items
end
b = Box.new(1, 2) { |n| p ["ctor", n] }
p b.items
