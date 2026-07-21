class Animal
  def speak(n); "sound" + n.to_s; end
  alias_method :vocalize, :speak
  alias cry speak
end
class Dog < Animal
  alias_method :woof, :speak
end
a = Animal.new
puts a.speak(2)
puts a.vocalize(3)
puts a.cry(1)
d = Dog.new
puts d.woof(4)
