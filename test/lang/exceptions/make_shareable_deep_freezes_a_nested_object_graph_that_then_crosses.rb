# A nested graph, object -> array -> object, every node frozen by one
# make_shareable, the whole graph then crossing a Ractor boundary by
# reference and dispatching on the far side.

class Node
  def initialize(v, child)
    @v = v
    @child = child
  end
  def v; @v; end
end
leaf = Node.new(3, nil)
root = Node.new(1, [leaf])
Ractor.make_shareable(root)
puts root.frozen?
puts leaf.frozen?
puts Ractor.shareable?(root)
sink = Ractor.new do
  got = Ractor.receive
  got.send(:v)
end
sink.send(root)
puts sink.value
__END__
true
true
true
1
#@ stderr
lang/exceptions/make_shareable_deep_freezes_a_nested_object_graph_that_then_crosses.rb:18: warning: Ractor API is experimental and may change in future versions of Ruby.
