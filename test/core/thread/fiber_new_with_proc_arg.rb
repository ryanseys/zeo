# `Fiber.new(&proc)` -- the block arriving as a proc argument rather than a
# literal block, the same conversion Thread.new already makes.
stepper = proc do
  Fiber.yield 1
  Fiber.yield 2
  3
end
f = Fiber.new(&stepper)
p f.resume
p f.resume
p f.resume

begin
  Fiber.new(&nil)
rescue ArgumentError => e
  puts e.message
end
__END__
1
2
3
tried to create Proc object without a block
