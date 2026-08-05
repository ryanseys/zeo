# Thread#[] / Thread#[]= are FIBER-local storage (thread_variable_get/set are
# the thread-local pair). A write inside a Fiber must be invisible after the
# fiber ends. zeo keeps one locals map per thread, so the fiber's write leaks
# out.
Thread.current[:v] = 5
puts Thread.current[:v]

f = Fiber.new do
  Thread.current[:v] = :fiber_only
  Thread.current[:v]
end
puts f.resume.inspect
puts Thread.current[:v].inspect

Thread.current.thread_variable_set(:tv, 7)
g = Fiber.new { Thread.current.thread_variable_get(:tv) }
puts g.resume
