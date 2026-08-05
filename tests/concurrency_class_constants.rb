# The concurrency classes answer as VALUES, and a SizedQueue is a real
# subclass of Queue.
#
# zeo's divergence: a SizedQueue instance answers `is_a?(SizedQueue)` and
# `instance_of?(SizedQueue)` FALSE while `is_a?(Queue)` stays true -- the
# instance carries Queue's class id (SizedQueue shares Queue's runtime
# representation, told apart only by its bound). Everything else here
# (constants as values, #name, #class, superclass) already matches.
# (Imported from spinel, which had the same observable, #3466.)
p Queue, Mutex, Thread, Fiber, ConditionVariable, SizedQueue
p Queue.new.class
p SizedQueue.new(2).class
p Mutex.new.class
p Fiber.new { 1 }.class
p Mutex.name, Thread.name, SizedQueue.name
q = SizedQueue.new(2)
p q.max
p q.is_a?(SizedQueue), q.instance_of?(SizedQueue), q.is_a?(Queue)
u = Queue.new
p u.is_a?(SizedQueue), u.instance_of?(Queue), u.is_a?(Queue)
p SizedQueue.superclass, Queue.superclass, Mutex.superclass
p Fiber.ancestors.first(2)
