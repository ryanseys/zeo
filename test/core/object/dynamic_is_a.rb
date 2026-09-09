# `is_a?` with the class held in a local or a parameter rather than named as
# a constant. The check walks the receiver's real ancestors either way, for a
# concrete class, an unrelated one, and an included module.

module Trainable
end

class Animal
end

class Dog < Animal
  include Trainable
end

class Cat < Animal
end

d = Dog.new
animal_klass = Animal
cat_klass = Cat
trainable_klass = Trainable

puts d.is_a?(animal_klass)     ? "dog-is-animal" : "dog-not-animal"
puts d.is_a?(cat_klass)        ? "dog-is-cat"    : "dog-not-cat"
puts d.is_a?(trainable_klass)  ? "dog-trainable" : "dog-not-trainable"

c = Cat.new
puts c.is_a?(animal_klass)     ? "cat-is-animal" : "cat-not-animal"
puts c.is_a?(trainable_klass)  ? "cat-trainable" : "cat-not-trainable"
__END__
dog-is-animal
dog-not-cat
dog-trainable
cat-is-animal
cat-not-trainable
