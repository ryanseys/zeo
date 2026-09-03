# methods / instance_methods reflection — names as Symbols, honoring
# visibility and inheritance.

module Walkable
  def walk; "walking"; end
end

class Animal
  def speak; end
  def legs; 4; end
  private
  def digest; end
end

class Dog < Animal
  include Walkable
  def bark; end
  def speak; "woof"; end        # override
  def self.breed; "mutt"; end
end

# instance_methods(false) — OWN methods only (overrides included), exact.
p Dog.instance_methods(false).sort
# default (inherited) — membership across the ancestry.
p Dog.instance_methods.include?(:speak)     # own override
p Dog.instance_methods.include?(:legs)      # inherited from Animal
p Dog.instance_methods.include?(:walk)      # from module
p Dog.instance_methods.include?(:digest)    # private → not listed

# visibility variants
p Animal.private_instance_methods(false)
p Animal.public_instance_methods(false).sort

# a module lists its own methods
p Walkable.instance_methods

# singleton (class) methods
p Dog.singleton_methods
p Dog.singleton_methods.include?(:breed)

# object-side: methods / respond_to? agree
d = Dog.new
p d.methods.include?(:bark)
p d.methods.include?(:walk)
p d.methods.include?(:breed)                 # false — a class method
p d.methods.include?(:frozen?)               # Kernel method
p d.respond_to?(:bark)
p d.respond_to?(:digest)                     # private → false
p d.respond_to?(:digest, true)               # include_all → true

# builtins expose (a subset of) their surface
p "hello".methods.include?(:upcase)
p [1, 2].methods.include?(:map)
p 42.methods.include?(:+)
__END__
[:bark, :speak]
true
true
true
false
[:digest]
[:legs, :speak]
[:walk]
[:breed]
true
true
true
false
true
true
false
true
true
true
true
