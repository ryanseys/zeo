# Test case/when, unless, next, default args

def describe(n)
  case n
  when 1
    "one"
  when 2, 3
    "two or three"
  when 4..6
    "four to six"
  else
    "other"
  end
end

puts describe(1)
puts describe(3)
puts describe(5)
puts describe(9)

# unless
x = 10
unless x > 20
  puts "small"
end

# next in loop
total = 0
i = 0
while i < 10
  i += 1
  next if i % 3 == 0
  total += i
end
puts total  # 1+2+4+5+7+8+10 = 37

# default args
def greet(name, greeting = "Hello")
  puts greeting
  puts name
end
greet("world")
greet("Ruby", "Hi")
