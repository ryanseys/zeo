# resume(v)'s v becomes the suspended Fiber.yield's return value;
# Fiber.yield(v)'s v becomes resume's return value -- CRuby
# `make_passing_arg` both ways.

g = Fiber.new do |x|
  y = Fiber.yield(x + 1)
  z = Fiber.yield(y + 10)
  z + 100
end
puts g.resume(5)
puts g.resume(6)
puts g.resume(7)
__END__
6
16
107
