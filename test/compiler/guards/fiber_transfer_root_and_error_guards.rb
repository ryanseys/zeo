# Fiber#transfer round-tripping through the root fiber, Fiber#[]/#[]=
# storage, the FiberError guards (yield in a transfer-entered fiber, double
# resume), and Fiber#kill running ensure blocks.

main = Fiber.current
f = Fiber.new do
  v = main.transfer(42)
  main.transfer(v + 1)
end
p f.transfer
p f.transfer(7)

Fiber[:tag] = :outer
g = Fiber.new { Fiber[:tag] }
p g.resume

begin
  ft = Fiber.new { |x| Fiber.yield x }
  ft.transfer(1)
rescue FiberError => e
  puts "transfer-yield: #{e.message}"
end

h = Fiber.new do
  begin
    Fiber.yield 1
  ensure
    puts "ensure ran"
  end
end
h.resume
p h.kill.is_a?(Fiber)
p h.alive?
__END__
42
8
:outer
transfer-yield: attempt to yield on a not resumed fiber
ensure ran
true
false
