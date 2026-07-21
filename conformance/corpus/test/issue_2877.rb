# A BFS keeping a visited Set compiled: `node == goal` (node poly, from
# queue.shift) no longer widens Set#=='s `other` param to Symbol via the
# poly-comparison arg binding, so Set#subset?'s `other.include?` stays typed.
require "set"

GRAPH = {
  a: [:b, :c],
  b: [:a, :d, :e],
  c: [:a, :f],
  d: [:b],
  e: [:b, :f],
  f: [:c, :e],
}

def shortest_path(graph, start, goal)
  queue = [start]
  visited = Set[start]
  parent = {}
  until queue.empty?
    node = queue.shift
    break if node == goal
    graph[node].each do |nbr|
      next if visited.include?(nbr)
      visited.add(nbr)
      parent[nbr] = node
      queue << nbr
    end
  end

  path = [goal]
  path.unshift(parent[path.first]) while parent.key?(path.first)
  path
end

p shortest_path(GRAPH, :a, :f)
p shortest_path(GRAPH, :d, :c)
p shortest_path(GRAPH, :a, :a)
