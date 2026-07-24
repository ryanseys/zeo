# UnboundMethod#bind_call(recv, *args) = bind(recv).call(*args) (#3246).
class C
  def a(n) = n + 1
  def greet(x, y = "!") = "#{x}#{y}"
end
p(C.instance_method(:a).bind_call(C.new, 5))
p(C.instance_method(:greet).bind_call(C.new, "hi"))
p(C.instance_method(:greet).bind_call(C.new, "hi", "?"))
