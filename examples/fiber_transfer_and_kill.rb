# Fiber#transfer round-tripping through the root fiber, Fiber#[]/#[]= storage,
# the FiberError guards, and Fiber#kill running ensure blocks.
main = Fiber.current

f = Fiber.new do
  acc = 0
  n = 0
  while n < 3
    got = main.transfer(acc)
    acc += got
    n += 1
  end
  main.transfer(acc)
end
p f.transfer        # 0
p f.transfer(10)    # 10
p f.transfer(5)     # 15
p f.transfer(100)   # 115

# Inheritable fiber-local storage.
Fiber[:tag] = :outer
g = Fiber.new { Fiber[:tag] }
p g.resume          # :outer

# A fiber entered via #transfer has no resumer, so Fiber.yield raises.
begin
  ft = Fiber.new { |x| Fiber.yield x }
  ft.transfer(1)
rescue FiberError => e
  puts "transfer-yield: #{e.message}"
end

# Resuming a fiber still running below us is a double resume.
$f1 = nil
$f2 = nil
$f1 = Fiber.new { $f2.resume }
$f2 = Fiber.new { $f1.resume }
begin
  $f1.resume
rescue FiberError => e
  puts "double-resume: #{e.message}"
end

# #kill runs ensure, is not caught by a bare rescue, and leaves the fiber dead.
h = Fiber.new do
  begin
    Fiber.yield 1
    Fiber.yield 2
  rescue => e
    puts "SHOULD NOT RESCUE"
  ensure
    puts "ensure ran"
  end
end
h.resume
p h.kill.is_a?(Fiber)
p h.alive?
puts "done"
