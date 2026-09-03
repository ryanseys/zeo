# Before this fix, `emit_super_inline` spliced the parent's body without
# ever binding its parameter names -- this only "worked" when parent/
# child happened to share names. Here they deliberately DON'T (`name`
# vs. `name, breed`), so this only passes once `super(name)` actually
# binds Animal's own `name` parameter fresh.

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

d = Dog.new("Rex", "Lab")
puts d.name
puts d.breed
__END__
Rex
Lab
