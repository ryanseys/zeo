class C
  def a(x, y); x + y; end
  def b(x, y=1); end
  def d(x, y:, z: 2); end
  def f(x, *r, z, k:, **o, &blk); end
  def g; end
end
o = C.new
puts o.method(:a).arity                 # 2
puts o.method(:a).parameters.inspect     # [[:req, :x], [:req, :y]]
puts o.method(:b).arity                 # -2
puts o.method(:d).arity                 # 2
puts o.method(:d).parameters.inspect     # [[:req,:x],[:keyreq,:y],[:key,:z]]
puts o.method(:f).arity                 # -4
puts o.method(:g).arity                 # 0
um = C.instance_method(:a)
puts um.class                           # UnboundMethod
puts um.name                            # a
puts um.arity                           # 2
puts um.bind(o).call(2, 3)              # 5
puts um.bind_call(o, 4, 5)              # 9
puts o.method(:a).unbind.class          # UnboundMethod
begin
  um.bind(42)
rescue TypeError
  puts "typeerror"
end
__END__
2
[[:req, :x], [:req, :y]]
-2
2
[[:req, :x], [:keyreq, :y], [:key, :z]]
-4
0
UnboundMethod
a
2
5
9
UnboundMethod
typeerror
