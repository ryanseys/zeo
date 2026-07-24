# A Thread (modelled as a single-threaded Fiber) carried through a container:
# #value/#join must dispatch on the boxed Fiber, not degrade to nil (#1261).
ts = (1..3).map { |i| Thread.new { i * 10 } }
p ts.map { |t| t.value }
p ts[0].value            # idempotent: re-reading a finished thread's value
arr = [Thread.new { 99 }]
p arr[0].value
m = Mutex.new
n = 0
m.synchronize { n += 1 }
puts n
