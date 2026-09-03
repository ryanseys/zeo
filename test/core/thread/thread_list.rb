# Thread.list returns the main thread plus every live spawned thread.
p Thread.list.size                       # 1 (just main)
p Thread.list.include?(Thread.current)   # true

# The three are HELD blocked while they are counted. A thread that has run to
# the end leaves the registry whether or not anyone joined it, so counting
# threads that race the count would count luck -- zeo runs real OS threads,
# where they finish before the next line.
gate = Queue.new
threads = (1..3).map { Thread.new { gate.pop } }
sleep 0.005 until threads.all? { |t| t.status == "sleep" }
p Thread.list.size                       # 4 (main + 3 blocked)

3.times { gate << :go }
threads.each(&:join)
p Thread.list.size                       # 1 (the spawned threads finished)
p Thread.list.include?(Thread.main)      # true
__END__
1
true
4
1
true
