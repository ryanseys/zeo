# Module#method_defined?(:sym) — compile-time decided via the
# class's recorded method table (walks the parent chain, includes
# attr readers/writers).
class Animal
  def name; "Animal"; end
  attr_accessor :age
end

class Dog < Animal
  def bark; "woof"; end
end

p Animal.method_defined?(:name)
p Animal.method_defined?(:age)
p Animal.method_defined?(:age=)
p Animal.method_defined?(:bark)
p Animal.method_defined?(:nope)

p Dog.method_defined?(:name)
p Dog.method_defined?(:bark)
p Dog.method_defined?(:age)
p Dog.method_defined?(:nope)

# String arg form
p Animal.method_defined?("name")
p Animal.method_defined?("nope")

# Inherit=false — restrict the lookup to the receiver's own methods.
# Dog#bark is defined locally; Dog inherits #name and #age from Animal.
# With inherit=false, only :bark is reported on Dog.
p Dog.method_defined?(:bark, false)   # true  (defined on Dog)
p Dog.method_defined?(:name, false)   # false (inherited from Animal)
p Dog.method_defined?(:age, false)    # false (inherited)
p Dog.method_defined?(:age=, false)   # false (inherited)
p Animal.method_defined?(:name, false) # true (defined on Animal)
p Animal.method_defined?(:bark, false) # false (defined on Dog only)
