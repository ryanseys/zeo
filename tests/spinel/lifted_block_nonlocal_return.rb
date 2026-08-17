# A `return` inside a block that is lifted to a proc.
#
# `@graph.adj[v].each { }` has a poly receiver, so the dispatch materializes
# the block as a real proc. The `return` in it must leave the METHOD, not just
# the proc: without a proc-return frame the value went to the proc's own return
# slot and the search kept looping, answering -1 for every reachable target.
#
# The destructuring `v, dist = queue.shift` writes a local the lifted block
# captures, so its target is a heap cell rather than a plain C local.

class Graph
  attr_reader :adj, :vertices
  def initialize(n)
    @vertices = n
    @adj = Array.new(n) { [] }
  end
  def add(a, b)
    @adj[a] << b
    @adj[b] << a
  end
  def each(&blk)
    @adj.each(&blk)
  end
end

class Other
  attr_reader :adj, :vertices
  def initialize; @adj = []; @vertices = 0; end
  def each; yield 1; end
end

class Solver
  def initialize(g)
    @graph = g
  end
  def bfs(start, target)
    return 0 if start == target
    visited = Array.new(@graph.vertices, 0)
    queue = [[start, 0]]
    visited[start] = 1
    while !queue.empty?
      v, dist = queue.shift
      @graph.adj[v].each do |neighbor|
        if neighbor == target
          return dist + 1
        end
        if visited[neighbor] == 0
          visited[neighbor] = 1
          queue << [neighbor, dist + 1]
        end
      end
    end
    -1
  end
end

def pick(f)
  f ? Graph.new(6) : Other.new
end

g = pick(true)
g.add(0, 1); g.add(1, 2); g.add(2, 3); g.add(0, 4); g.add(4, 5)
s = Solver.new(g)
p s.bfs(0, 3)
p s.bfs(0, 5)
p s.bfs(0, 0)
