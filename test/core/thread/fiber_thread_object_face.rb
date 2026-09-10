# The concurrency handles are heap instances, so Object's identity face works
# on them: equal? / eql? / == / != are pointer comparison and frozen? is
# false.
#
# zeo's divergences: (1) `x.equal?(x)` answers FALSE for Mutex, Queue and
# SizedQueue -- each read of the variable hands out a fresh box and equal?
# compares the box rather than the handle behind it (ConditionVariable,
# Fiber and Thread compare the handle and answer true); (2) Fiber#raise on a
# fiber that has never been resumed delivers the requested RuntimeError
# instead of ruby's FiberError, since there is nowhere to deliver it yet.
a = Mutex.new
b = Queue.new
c = SizedQueue.new(2)
d = ConditionVariable.new
e = Fiber.new { 1 }
p a.equal?(a), a.eql?(a), a == a
p b.equal?(b), b.eql?(b)
p c.equal?(c)
p d.equal?(d)
p e.equal?(e), e.eql?(e), e == e
p e != Fiber.new { 2 }
p a.frozen?, e.frozen?
p a.equal?(b)

# Fiber#raise on a fiber that has never run has nowhere to deliver, so it is a
# FiberError -- not the requested exception raised in the caller. #3468.
f = Fiber.new { Fiber.yield 1 }
p (f.raise(RuntimeError, "boom") rescue $!.class)
g = Fiber.new { Fiber.yield 1; :after }
g.resume
p (g.raise(RuntimeError, "boom") rescue $!.class)
p Fiber.new { Fiber.yield 1 }.kill.class

# Fiber#resume passes every argument to the block's parameters. The fiber
# carries one value, so two or more are packed and the body unpacks them. #3469.
h = Fiber.new { |x, y| x + y }
p h.resume(3, 4)
i = Fiber.new { |*r| r }
p i.resume(1, 2)
j = Fiber.new { |x, y| Fiber.yield(x); y }
p j.resume(5, 6)
p j.resume
p Fiber.new { |x| x * 2 }.resume(21)
p Fiber.new { 99 }.resume
__END__
true
true
true
true
true
true
true
true
true
true
true
false
false
false
FiberError
RuntimeError
Fiber
7
[1, 2]
5
6
42
99
