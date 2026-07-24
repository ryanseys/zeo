# Test Phase 2: heterogeneous arrays, bimorphic dispatch

# Heterogeneous array
arr = [1, "two", 3.0, true, nil]
arr.each do |x|
  puts x
end
# 1, two, 3.0, true, (blank)

# Array with mixed types
nums = [10, 20, 30]
puts nums.length  # 3

# Duck typing (bimorphic)
class Dog
  def speak
    "Woof!"
  end
end

class Cat
  def speak
    "Meow!"
  end
end

def make_noise(animal)
  puts animal.speak
end

make_noise(Dog.new)  # Woof!
make_noise(Cat.new)  # Meow!

puts "done"
