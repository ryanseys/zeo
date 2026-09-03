def double(x) = x * 2
m = method(:double)
p m.call(21)
p m.(21)
p m[21]
p m.name
p [1, 2].map(&method(:double))

class Greeter
  def initialize(g) = @g = g
  def greet(name) = "#{@g}, #{name}!"
end
g = Greeter.new("Hello")
gm = g.method(:greet)
p gm.call("Ada")
p gm.receiver.equal?(g)
p "hello".method(:upcase).call
__END__
42
42
42
:double
[2, 4]
"Hello, Ada!"
true
"HELLO"
