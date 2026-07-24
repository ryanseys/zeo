def double(x)
  x * 2
end

# Direct call for type inference
puts double(1)   # 2

m = method(:double)
puts m.call(5)   # 10
puts m.call(10)  # 20

def add_ten(x)
  x + 10
end

puts add_ten(0)  # 10

a = method(:add_ten)
puts a.call(3)   # 13

puts "done"
