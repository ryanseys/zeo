module Walks; end
module Swims; end
class Animal; end
class Dog < Animal; include Walks; end
p Dog.include?(Walks)
p Dog.include?(Swims)
p Animal.include?(Walks)
