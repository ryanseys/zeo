class Animal
  def speak
    puts :generic
  end
end

class Dog < Animal
  def speak
    super
    puts :woof
  end
end

Dog.new.speak
