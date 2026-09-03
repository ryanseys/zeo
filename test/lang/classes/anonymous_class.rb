# Anonymous runtime classes (#97): Class.new with define_method and def bodies,
# constant naming, is_a?/instance_of?, and a runtime superclass chain.

Animal = Class.new do
  define_method(:speak) { "generic sound" }
  def legs
    4
  end
end
a = Animal.new
puts a.speak
puts a.legs
puts a.is_a?(Animal)
puts a.is_a?(Object)
puts a.instance_of?(Animal)
puts Animal.superclass
puts Animal.name

Mammal = Class.new(Animal) do
  define_method(:warm_blooded) { true }
end
m = Mammal.new
puts m.speak
puts m.legs
puts m.warm_blooded
puts m.is_a?(Animal)
puts m.instance_of?(Animal)
puts Mammal.superclass.name

klass = Class.new
puts klass.new.is_a?(klass)
__END__
generic sound
4
true
true
true
Object
Animal
generic sound
4
true
true
false
Animal
true
