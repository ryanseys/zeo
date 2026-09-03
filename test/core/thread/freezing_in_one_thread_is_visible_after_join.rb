# Threads share the heap (no Ractor boundary): a freeze in one is the
# same AtomicBool every other execution context reads.

s = "x"
t = Thread.new { s.freeze }
t.join
puts s.frozen?
__END__
true
