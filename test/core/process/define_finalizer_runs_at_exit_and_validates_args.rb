# The finalizer for a still-referenced object runs at program exit; the
# undefined one never runs; the bad-argument shapes match CRuby.

s = Object.new
ObjectSpace.define_finalizer(s, proc { |id| puts "finalized(#{id.class})" })
t = Object.new
ObjectSpace.define_finalizer(t) { |id| puts "block finalizer" }
u = Object.new
ObjectSpace.define_finalizer(u, proc { puts "SHOULD NOT RUN" })
ObjectSpace.undefine_finalizer(u)
begin
  ObjectSpace.define_finalizer("x")
rescue ArgumentError => e
  puts "e1: #{e.message}"
end
begin
  ObjectSpace.define_finalizer("x", 5)
rescue ArgumentError => e
  puts "e2: #{e.message}"
end
puts "before exit"
__END__
e1: tried to create Proc object without a block
e2: wrong type argument Integer (should be callable)
before exit
finalized(Integer)
block finalizer
