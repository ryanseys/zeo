# A blockless `Thread.new`/`Fiber.new` raises at runtime in real Ruby
# (thread.c:1034) rather than being a static error, and both are
# rescuable -- so the program must compile and run.

def err
  yield
rescue => e
  "#{e.class}: #{e.message}"
end
puts err { Thread.new }
puts err { Fiber.new }
t = Thread.new { 7 }
p t.value
__END__
ThreadError: must be called with a block
ArgumentError: tried to create Proc object without a block
7
