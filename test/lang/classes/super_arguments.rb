class Animal
  def initialize(name)
    @name = name
  end

  def name
    @name
  end
end

class Dog < Animal
  def initialize(name, breed)
    super(name)
    @breed = breed
  end

  def breed
    @breed
  end
end

class Puppy < Animal
  def initialize(name)
    super
    @greeting = "woof from #{name}"
  end

  def greeting
    @greeting
  end
end

d = Dog.new("Rex", "Lab")
puts d.name
puts d.breed

pup = Puppy.new("Fido")
puts pup.name
puts pup.greeting
__END__
Rex
Lab
Fido
woof from Fido
