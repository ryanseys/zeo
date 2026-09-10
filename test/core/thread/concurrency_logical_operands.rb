# `&&` / `||` with a concurrency handle on either side must keep the handle's
# full identity.
#
# zeo's divergence is one probe: a Queue carried through `||` no longer
# inspects in the `#<Thread::Queue:0x...>` form (the final
# `inspect.start_with?` answers false). Every other operand shape -- classes,
# values, nil, locked? -- already matches.
m = Mutex.new
p(m.lock && m.locked?)
m.unlock

q = Queue.new
sq = SizedQueue.new(2)
cv = ConditionVariable.new
f = Fiber.new { 1 }
t = Thread.new { 1 }
t.join

p (:ok && q).class
p (:ok && sq).class
p (:ok && m).class
p (:ok && cv).class
p (:ok && f).class
p (:ok && t).class

p (q || :ok).class
p (m || :ok).class
p (f || :ok).class

p (m && 1)
p (f || 2).inspect.start_with?("#<Fiber:")
p (q && :done)

# the handle on the left with each of the other operand shapes
p (m && "s")
p (m && 1.5)
p (m && [1])
p (m && nil).nil?
p (m && m.locked?)

# a boxed handle names itself in inspect rather than answering #<Object>
p (q || 2).inspect.start_with?("#<Thread::Queue:")

# a falsy left keeps the left, and a nil-valued handle reads falsy
p (nil && q).nil?
p (nil || q).class
__END__
true
Thread::Queue
Thread::SizedQueue
Thread::Mutex
Thread::ConditionVariable
Fiber
Thread
Thread::Queue
Thread::Mutex
Fiber
1
true
:done
"s"
1.5
[1]
true
false
true
true
Thread::Queue
