# Class#ancestors and Class#superclass over the full chain (user + builtin),
# and Class values rendering through inspect/p.
class Animal; end
class Dog < Animal; end

p Dog.ancestors
p Animal.ancestors
p Dog.superclass
p Integer.superclass
p Float.superclass
p Integer.ancestors
p String.ancestors
p Dog
p Integer
# nested in an array (poly inspect of a Class element)
p [Dog, Animal, Integer]
# superclass chaining
puts Dog.superclass.superclass     # Object
puts Integer.superclass.superclass # Object
# <= / ancestors-driven comparisons still hold
puts(Dog <= Animal)                # true
puts(Dog <= Object)                # true
puts(Integer <= Numeric)           # true
__END__
[Dog, Animal, Object, Kernel, BasicObject]
[Animal, Object, Kernel, BasicObject]
Animal
Numeric
Numeric
[Integer, Numeric, Comparable, Object, Kernel, BasicObject]
[String, Comparable, Object, Kernel, BasicObject]
Dog
Integer
[Dog, Animal, Integer]
Object
Object
true
true
true
