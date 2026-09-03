# A wait only another Ruby thread can end, with no other Ruby thread left to
# end it, is a deadlock. zeo used to HANG there; CRuby raises `fatal`.
#
# The check rides the poll the blocking waits already do. A wait registers
# itself, and when every live thread is registered none of them can be the one
# to push, unlock or send -- so nothing can make progress. The verdict needs
# the condition to hold across two consecutive polls, because a thread between
# two waits is briefly absent from the count without being runnable in any
# useful sense, and one poll would call that a deadlock.
#
# Only UNTIMED waits count. `sleep 2` ends by itself, so a thread inside one is
# not blocked on anybody, and CRuby draws the same line.
#
# `fatal` descends straight from `Exception`, so `rescue => e` misses it and
# `rescue Exception` catches it. Its lower-case name cannot be written in Ruby
# source at all -- a constant must start upper-case, and
# `Object.const_defined?("fatal")` raises `wrong constant name` on ruby too --
# so the class is reachable only through `e.class`.
#
# DIVERGENCE, deliberate: CRuby's message continues past the first line with a
# thread dump -- native addresses, `rb_thread_t` pointers, a per-thread
# backtrace. Those describe MRI's VM, are different on every run, and no
# program can act on them. zeo's message is the first line, which is the part
# that describes the program, and it is what this golden compares. The
# uncaught render matches too:
# `file:LINE:in 'Thread::Queue#pop': No live threads left. Deadlock? (fatal)`.

def attempt
  yield
  :no_raise
rescue StandardError => e
  [:standard, e.class]
rescue Exception => e
  # The FIRST LINE only -- see the divergence note above.
  [:exception, e.class, e.message.lines.first.chomp]
end

# An empty Queue with nobody to push to it.
p attempt { Queue.new.pop }

# A producer exists, so the wait ends normally and the check must NOT fire.
q = Queue.new
producer = Thread.new { sleep 0.05; q << 42 }
p q.pop
producer.join

# The consumer blocks first and the main thread pushes later -- the shape a
# one-poll verdict would call a deadlock.
slow = Queue.new
consumer = Thread.new { slow.pop }
sleep 0.05
slow << 7
p consumer.value

# A timed sleep is not a deadlock: it ends by itself.
p attempt { sleep 0.01 }

# Several workers blocked while the MAIN thread is runnable and has never
# waited for anything -- minitest's parallel executor, and the shape that
# caught the first version of this check. The denominator has to be the live
# RUBY thread count: a scheduling context is installed lazily, the first time
# a thread waits, so counting contexts makes a runnable thread invisible and
# declares a deadlock while the thread about to push is still working.
work = Queue.new
results = Queue.new
workers = 3.times.map { Thread.new { results << work.pop * 2 } }
busy = 0
200_000.times { |i| busy += i % 3 }
3.times { |i| work << i }
p workers.each(&:join).size
p 3.times.map { results.pop }.sum
p busy > 0
__END__
[:exception, fatal, "No live threads left. Deadlock?"]
42
7
:no_raise
3
6
true
