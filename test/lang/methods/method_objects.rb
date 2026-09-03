# `method(:name)` answers a Method -- a bound (receiver, name) pair whose
# #call dispatches through the ordinary send machinery.

def double(x) = x * 2

m = method(:double)
p m.call(21)
p m.(21)
p m[21]
p m.name
p m.class

# A Method converts to a Proc, so it passes as a block.
p [1, 2, 3].map(&method(:double))
p method(:double).to_proc.call(5)
p method(:double).to_proc.class

# On an explicit receiver.
class Greeter
  def initialize(greeting)
    @greeting = greeting
  end

  def greet(name)
    "#{@greeting}, #{name}!"
  end

  def shout(name)
    greet(name).upcase
  end
end

g = Greeter.new("Hello")
gm = g.method(:greet)
p gm.call("Ada")
p gm.name
p gm.receiver.equal?(g)

# The binding is to THAT instance, so it keeps its own ivars.
other = Greeter.new("Hi")
p other.method(:greet).call("Bob")
p gm.call("Bob")

# Method objects are first-class: store them, pass them, call later.
handlers = [g.method(:greet), g.method(:shout)]
handlers.each { |h| puts h.call("Cat") }

table = { greet: g.method(:greet), shout: g.method(:shout) }
p table[:shout].call("dog")

# A Method for a builtin receiver's method.
up = "hello".method(:upcase)
p up.call
len = [1, 2, 3].method(:length)
p len.call

# === invokes it, so a Method works in a case/when.
matcher = method(:double)
p matcher === 4

# A Method surviving through a block that yields it.
def with_method
  yield method(:double)
end
p(with_method { |mm| mm.call(50) })

# Method#inspect mentions the owner and name.
p g.method(:greet).inspect.include?("greet")

# An unknown method name raises NameError at CONSTRUCTION, not at call.
begin
  "str".method(:definitely_not_defined)
rescue NameError => e
  puts e.message
end
begin
  g.method(:definitely_not_defined)
rescue NameError => e
  puts e.message
end

# A PRIVATE method is still reachable by reflection (privacy limits call
# sites, not `method`).
class WithPrivate
  private

  def secret = "shh"
end
p WithPrivate.new.method(:secret).call
__END__
42
42
42
:double
Method
[2, 4, 6]
10
Proc
"Hello, Ada!"
:greet
true
"Hi, Bob!"
"Hello, Bob!"
Hello, Cat!
HELLO, CAT!
"HELLO, DOG!"
"HELLO"
3
8
100
true
undefined method 'definitely_not_defined' for class 'String'
undefined method 'definitely_not_defined' for class 'Greeter'
"shh"
