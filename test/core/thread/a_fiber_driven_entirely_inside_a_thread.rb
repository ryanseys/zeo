# The fiber table is per-OS-thread; a fiber created and resumed inside
# one Thread works because both operations run on that same OS thread.

t = Thread.new do
  ff = Fiber.new do |x|
    Fiber.yield x + 1
    99
  end
  a = ff.resume(1)
  b = ff.resume
  a + b
end
puts t.value
__END__
101
