# Fiber -- cooperative stackful coroutines

# a producer yielding values to its resumer
producer = Fiber.new do
  Fiber.yield 1
  Fiber.yield 2
  3
end
puts producer.alive?
puts producer.resume
puts producer.resume
puts producer.resume
puts producer.alive?

# values flow both directions: resume(v) becomes yield's return value
echo = Fiber.new do |x|
  y = Fiber.yield(x + 1)
  y * 10
end
puts echo.resume(5)
puts echo.resume(7)

# first-resume arguments bind to the block's parameters
adder = Fiber.new do |a, b|
  a + b
end
puts adder.resume(20, 22)

# a fiber shares its enclosing scope's locals across suspensions
total = 0
worker = Fiber.new do
  total += 1
  Fiber.yield
  total += 10
end
worker.resume
puts total
worker.resume
puts total

# a dead fiber cannot be resumed
begin
  producer.resume
rescue FiberError => e
  puts e.send(:message)
end

# an exception inside a fiber surfaces at the resumer
risky = Fiber.new do
  raise "kaboom"
end
begin
  risky.resume
rescue RuntimeError => e
  puts "rescued: #{e.send(:message)}"
end
puts risky.alive?
