class Box
  def initialize(v)
    @v = v
  end
  def v
    @v
  end
end
b = Box.new(41)
Ractor.make_shareable(b)
puts b.frozen?
sink = Ractor.new do
  got = Ractor.receive
  got.send(:v)
end
sink.send(b)
puts sink.value + 1
__END__
true
42
#@ stderr
core/thread/a_deeply_frozen_object_crosses_a_ractor_boundary_by_reference.rb:12: warning: Ractor API is experimental and may change in future versions of Ruby.
