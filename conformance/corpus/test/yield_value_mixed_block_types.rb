# A class-method with `yield` inlined at two call sites whose blocks return
# DIFFERENT types: the yield value must be boxed by each call site's own block
# type, not the method AST node's single cached type (which segfaulted by
# boxing an int result as a string).
class Box
  def initialize(s)
    @s = s
  end
  def content
    @s
  end
  def self.make(s)
    b = Box.new(s)
    if block_given?
      yield b
    else
      b
    end
  end
end

r = Box.make("foo") { |b| b.content }
n = Box.make("hello") { |b| b.content.length }
p r
p n
m = Box.make("tail")
p m.content
