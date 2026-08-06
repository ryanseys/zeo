require "tsort"

class Deps
  include TSort
  def initialize(h) = @h = h
  def tsort_each_node(&b) = @h.each_key(&b)
  def tsort_each_child(node, &b) = @h.fetch(node, []).each(&b)
end

graph = Deps.new({
  "app"  => ["lib", "util"],
  "lib"  => ["util"],
  "util" => [],
})
p graph.tsort
p graph.strongly_connected_components.sort

cyclic = Deps.new({ "a" => ["b"], "b" => ["a"] })
begin
  cyclic.tsort
rescue TSort::Cyclic => e
  puts "cyclic: #{e.class}"
end
# The module-function form takes the two walks as callables.
edges = { 1 => [2], 2 => [] }
each_node  = ->(&b) { edges.each_key(&b) }
each_child = ->(n, &b) { edges[n].each(&b) }
p TSort.tsort(each_node, each_child)
