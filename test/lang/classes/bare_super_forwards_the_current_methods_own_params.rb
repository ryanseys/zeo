# Bare `super` (no parens) forwards the CURRENT method's own already-
# bound parameter values positionally -- also unbound before this fix
# (same root cause as the explicit-args case above).

class Animal
  def initialize(name)
    @name = name
  end
  def name
    @name
  end
end

class Dog < Animal
  def initialize(name)
    super
    @greeting = "woof from #{name}"
  end
  def greeting
    @greeting
  end
end

d = Dog.new("Rex")
puts d.name
puts d.greeting
__END__
Rex
woof from Rex
