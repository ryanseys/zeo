# Runtime metaprogramming (#97): define_method with a computed name, inside a
# class-body loop whose block captures the loop variable, plus an
# explicit-receiver define_method and define_singleton_method.

class Animal
  [:walk, :run, :swim].each do |action|
    define_method(action) { "#{action}ing" }
  end
end

a = Animal.new
puts a.walk
puts a.run
puts a.swim
puts a.respond_to?(:walk)
puts a.respond_to?(:fly)

# Explicit-receiver define_method adds a method dynamically.
Animal.define_method(:name) { "generic animal" }
puts Animal.new.name

# define_singleton_method on a specific object.
dog = Animal.new
dog.define_singleton_method(:bark) { "woof" }
puts dog.bark
puts Animal.new.respond_to?(:bark)

# define_singleton_method on a class -> a class method.
Animal.define_singleton_method(:kingdom) { "Animalia" }
puts Animal.kingdom
