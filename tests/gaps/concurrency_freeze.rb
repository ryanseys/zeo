# A Mutex, ConditionVariable, Fiber or Thread is an ordinary heap instance
# and freezes like one; a Queue is the exception Ruby itself makes (freezing
# one raises, since a frozen queue could never be pushed to again).
#
# zeo's divergence: `freeze` on a Mutex/ConditionVariable is accepted but
# does not stick -- `frozen?` answers false immediately after. The handle
# types live outside the `__frozen` AtomicBool convention generated instances
# carry, so freeze has nowhere to record itself. (Spinel once had the same
# observable for its own reasons, #3483.)
a = Mutex.new
p a.frozen?
p a.freeze.equal?(a)
p a.frozen?

b = ConditionVariable.new
b.freeze
p b.frozen?

c = Fiber.new { 1 }
c.freeze
p c.frozen?

t = Thread.new { 1 }
t.join
t.freeze
p t.frozen?

o = Object.new
o.freeze
p o.frozen?

q = Queue.new
p q.frozen?
begin
  q.freeze
  p "no raise"
rescue TypeError => e
  p [e.class, e.message.start_with?("cannot freeze #<Thread::Queue:")]
end

s = SizedQueue.new(1)
begin
  s.freeze
  p "no raise"
rescue TypeError => e
  p [e.class, e.message.start_with?("cannot freeze #<Thread::SizedQueue:")]
end

# a frozen handle still works as itself
m = Mutex.new
m.freeze
m.lock
p m.locked?
m.unlock
p m.frozen?
