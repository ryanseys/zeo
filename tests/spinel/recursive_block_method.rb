# A method that drives its block (yield or block.call) and calls itself
# recursively compiles as a real function with a proc-materialized block
# instead of hitting the inline depth cap: the &block param rides as an
# sp_Proc * and the self-call forwards it, so the recursion is plain C
# recursion.

# class method, block.call + &block self-forward
class Walker
  def self.walk(depth, &block)
    block.call(depth)
    walk(depth - 1, &block) if depth > 0
  end
end

Walker.walk(2) { |n| puts n }

# instance method
class IWalker
  def walk(depth, &block)
    block.call(depth)
    walk(depth - 1, &block) if depth > 0
  end
end

IWalker.new.walk(2) { |n| puts n + 10 }

# top-level def
def twalk(depth, &block)
  block.call(depth)
  twalk(depth - 1, &block) if depth > 0
end

twalk(2) { |n| puts n + 20 }

# declared &block with a literal `yield` in the body
class YWalker
  def self.walk(depth, &block)
    yield depth
    walk(depth - 1, &block) if depth > 0
  end
end

YWalker.walk(2) { |n| puts n + 30 }

# recursion through a receiver's own yielding method (the tree-walk shape)
class TNode
  def initialize(depth)
    @depth = depth
  end

  def leaf?
    @depth <= 0
  end

  def each
    yield TNode.new(@depth - 1)
    yield TNode.new(@depth - 2)
    nil
  end
end

class TreeWalker
  def self.walk(node, &block)
    return block.call(node) if node.leaf?

    node.each do |child| walk(child, &block) end
  end
end

TreeWalker.walk(TNode.new(2)) { |n| puts "leaf" }

# caller passes a real proc
pr = proc { |n| puts n + 40 }
Walker.walk(1, &pr)

# a captured local mutated through the recursion
total = 0
Walker.walk(3) { |n| total += n }
puts total

# a yield-method inlined INSIDE the recursive method: the callee's own yield
# splices its call-site block, while a yield in that spliced block still binds
# the enclosing method's proc (callee even shadows the &block name)
class SNode
  def each
    block = 5
    yield block
    nil
  end
end

class MixWalker
  def self.walk(depth, &block)
    yield depth
    SNode.new.each do |x|
      walk(depth - 1, &block) if depth > 0
    end
    nil
  end
end

MixWalker.walk(1) { |n| puts n + 50 }
