class Array
  def size = 1
end
p [1, 2].size
class Array
  def size = 2
end
p [1, 2].size

module Kernel
  def helper = "first"
end
puts helper
module Kernel
  def helper = "second"
end
puts helper

class String
  def shout = "first"
end
puts "a".shout
class String
  def shout = "second"
end
puts "a".shout
