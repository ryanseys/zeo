# Method#inspect and #to_s open with the owner and the parameter names, and unbind answers an UnboundMethod that renders the same way.
# (spinel issue #3249)
def dbl(n) = n * 2
def add(a, b) = a + b

p method(:dbl).inspect.start_with?("#<Method: Object#dbl(n)")
p method(:dbl).to_s.start_with?("#<Method: Object#dbl(n)")
p method(:dbl).unbind.class
p method(:dbl).unbind.inspect.start_with?("#<UnboundMethod: Object#dbl(n)")
p method(:dbl).source_location.is_a?(Array)

begin
  (method(:dbl) << method(:add)).call(3)
rescue ArgumentError => e
  p e.message
end
begin
  (method(:dbl) >> method(:add)).call(5)
rescue ArgumentError => e
  p e.message
end
p (method(:add) >> method(:dbl)).call(3, 4) rescue p $!.class

class Widget
  def render(depth, pad = 0, *rest) = depth
end
p Widget.instance_method(:render).inspect.start_with?("#<UnboundMethod: Widget#render(depth, pad=..., *rest)")
p Widget.instance_method(:render).class

module Api
  def self.fetch(url) = url
end
p Api.method(:fetch).inspect.start_with?("#<Method: Api.fetch(url)")
__END__
true
true
UnboundMethod
true
true
"wrong number of arguments (given 1, expected 2)"
"wrong number of arguments (given 1, expected 2)"
14
true
UnboundMethod
true
