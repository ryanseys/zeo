# Fibers own separate stacks, allocated by the runtime the binary links.
fib = Fiber.new do
  a, b = 0, 1
  loop do
    Fiber.yield a
    a, b = b, a + b
  end
end
p 8.times.map { fib.resume }
__END__
[0, 1, 1, 2, 3, 5, 8, 13]
