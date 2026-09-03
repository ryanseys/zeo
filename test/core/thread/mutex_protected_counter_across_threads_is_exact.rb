# THE canonical threading idiom -- Proc-within-Proc (Thread.new wrapping
# synchronize). The mutex makes the sum exact (2000) regardless of how the
# two OS threads interleave.

m = Mutex.new
count = 0
t1 = Thread.new { 1000.times { m.synchronize { count += 1 } } }
t2 = Thread.new { 1000.times { m.synchronize { count += 1 } } }
t1.join
t2.join
puts count
__END__
2000
