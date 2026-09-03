class Animal
  def speak(sound) = "#{sound}!"
end
class Dog < Animal
end
d = Dog.new
m = d.method(:speak)
p m.owner
p m.original_name
shout = ->(s) { s.upcase }
p (m >> shout).call("woof")
p (m << shout).call("woof")
p (1.method(:+) >> ->(n) { n * 2 }).call(10)
p d.method(:speak) == d.method(:speak)
p Dog.new.method(:speak) == Dog.new.method(:speak)
p m.hash == d.method(:speak).hash
class Adder
  def add3(a, b, c) = a + b + c
end
p Adder.new.method(:add3).curry[1][2][3]
um = Dog.instance_method(:speak)
p um.owner
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
"bark!"
