# `path << [start, -1]` then `cell, _ = path[id]`. While the fixpoint runs,
# `start` has no type yet and the literal reads as an int array (an UNKNOWN
# element unifies away), so the container was narrowed to a table of int arrays
# -- and that pin outlived the rounds that knew `start` is an object. `cell`
# then bound an Integer and the call on it raised.

class C
  attr_reader :v, :ns
  def initialize(v); @v = v; @ns = []; end
  def link(c); @ns << c; end
end

def bfs(start, target)
  path = []
  queue = []
  path << [start, -1]
  queue << 0
  while !queue.empty?
    pid = queue.shift
    cell, _ = path[pid]
    cell.ns.each do |n|
      return path.length if n == target
      path << [n, pid]
      queue << path.size - 1
    end
  end
  -1
end

a = C.new(1)
b = C.new(2)
a.link(b)
p bfs(a, b)
