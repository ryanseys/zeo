# Test: nil-initialized ivar type inference
# When @left/@right are initialized to nil and later assigned Node objects,
# the compiler should infer them as OBJECT(Node), not NIL.

class Node
  attr_accessor :left, :right, :val
  def initialize(v)
    @val = v
    @left = nil
    @right = nil
  end
end

class Tree
  attr_accessor :root
  def initialize
    @root = nil
  end
  def set_root(n)
    @root = n
  end
end

def make_node(depth)
  n = Node.new(depth)
  if depth > 0
    child_l = make_node(depth - 1)
    child_r = make_node(depth - 1)
    n.left = child_l
    n.right = child_r
  end
  n
end

tree = make_node(10)
puts tree.val
puts tree.left.val
puts tree.right.val

t = Tree.new
t.set_root(tree)
puts t.root.val

puts "done"
