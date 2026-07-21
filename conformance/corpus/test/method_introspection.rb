# Test __method__, freeze/frozen?

# __method__
def greet
  puts __method__  # greet
end
greet

def calculate
  puts __method__  # calculate
end
calculate

# freeze / frozen? (AOT treats everything as frozen)
s = "hello"
s.freeze
puts s           # hello
puts s.frozen?   # true

# Integer#freeze (no-op)
n = 42
puts n.frozen?   # true

puts "done"
