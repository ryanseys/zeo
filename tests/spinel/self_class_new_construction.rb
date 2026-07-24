# `self.class.new(args)` in an instance method of a leaf class (no
# subclasses) constructs the enclosing class statically — the
# idiomatic functional-update pattern (e.g. AST::Node#updated).
class Node
  attr_reader :children
  def initialize(children = [])
    @children = children.to_a.freeze
  end
  def with(more)
    self.class.new(@children + more)
  end
end
n = Node.new([Node.new, Node.new])
m = n.with([Node.new])
puts m.children.size
puts n.children.size
