# GCBench (from yjit-bench) - binary tree construction benchmark

class Node
  attr_accessor :left, :right, :i, :j
  def initialize
    @left = nil
    @right = nil
    @i = 0
    @j = 0
    @tag = "n"
  end
end

STRETCH_DEPTH = 18
LONG_LIVED_DEPTH = 16
MIN_DEPTH = 4
MAX_DEPTH = 16

def tree_size(depth)
  (1 << (depth + 1)) - 1
end

def num_iters(depth)
  2 * tree_size(STRETCH_DEPTH) / tree_size(depth)
end

def populate(depth, node)
  if depth > 0
    node.left = Node.new
    node.right = Node.new
    populate(depth - 1, node.left)
    populate(depth - 1, node.right)
  end
end

def make_tree(depth)
  if depth <= 0
    Node.new
  else
    n = Node.new
    n.left = make_tree(depth - 1)
    n.right = make_tree(depth - 1)
    n
  end
end

def time_construction(depth)
  n = num_iters(depth)
  i = 0
  while i < n
    node = Node.new
    populate(depth, node)
    i = i + 1
  end
  i = 0
  while i < n
    make_tree(depth)
    i = i + 1
  end
end

make_tree(STRETCH_DEPTH)

long_lived = Node.new
populate(LONG_LIVED_DEPTH, long_lived)

depth = MIN_DEPTH
while depth <= MAX_DEPTH
  time_construction(depth)
  depth = depth + 2
end

puts "done"
