# Issue #404 Phase 3 Tier 3. Dynamic is_a?(klass) where the
# klass argument is a sp_Class value (variable or parameter)
# rather than a ConstantReadNode. Pre-Tier-3 the codegen
# read @nd_name on the arg as the bare class name and so
# couldn't resolve a Class-typed local. Tier 3 routes through
# sp_class_le at runtime so the hierarchy check happens
# against whichever class the variable carries.
#
# Coverage:
#   - Typed recv (obj_Dog) with dynamic klass parameter.
#     Routes through compile_introspection_expr's new dynamic
#     arg arm; sp_class_le over the precomputed ancestors.
#   - Typed recv with concrete vs unrelated klass.
#   - Same recv with a module-typed klass (Tier 2 +
#     ancestors-table glue).

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
