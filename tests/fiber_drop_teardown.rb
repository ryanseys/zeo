# Suspended fibers and abandoned enumerator iterations are torn down
# without running their `ensure` bodies -- at process end, at thread end,
# and when a re-init row (`rewind`) discards a suspended iteration
# mid-program. CRuby parity: a never-finished fiber runs no ensure.

e = [1, 2, 3].to_enum
p e.next
e.rewind
p e.next

f = Fiber.new do
  Fiber.yield :a
ensure
  puts "F-ENSURE"
end
p f.resume

t = Thread.new do
  g = Fiber.new do
    Fiber.yield :b
  ensure
    puts "T-ENSURE"
  end
  p g.resume
end
t.join

puts "done"
