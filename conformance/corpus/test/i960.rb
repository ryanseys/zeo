# Kernel#method returns a Method object (not Integer 0). .class and
# .name dispatch; for a top-level user method, .call works too.
m = method(:puts)
puts m.class
puts m.name

def dbl(x)
  x * 2
end
d = method(:dbl)
puts d.class
puts d.name
puts d.call(21)
