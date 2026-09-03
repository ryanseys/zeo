class Animal
  def speak(sound) = "#{sound}!"
end
class Dog < Animal
end

d = Dog.new
m = d.method(:speak)

# owner walks to the defining ancestor, not the receiver's class
p m.owner
p m.original_name

# composition: (m >> f).call(x) == f(m.call(x))
shout = ->(s) { s.upcase }
p (m >> shout).call("woof")
p (m << shout).call("woof")

# a Method composes with another Method too
add1 = 1.method(:+)
double = ->(n) { n * 2 }
p (add1 >> double).call(10)

# equality: same receiver + same method are ==, different receivers are not
p d.method(:speak) == d.method(:speak)
p Dog.new.method(:speak) == Dog.new.method(:speak)
p m.hash == d.method(:speak).hash

# curry a multi-arg Method
class Adder
  def add3(a, b, c) = a + b + c
end
cm = Adder.new.method(:add3)
p cm.curry[1][2][3]

# UnboundMethod owner / original_name
um = Dog.instance_method(:speak)
p um.owner
p um.original_name
p um.bind(Dog.new).call("bark")
__END__
Animal
:speak
"WOOF!"
"WOOF!"
22
true
false
true
6
Animal
:speak
"bark!"
