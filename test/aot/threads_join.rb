# Native threads in a linked binary: each thread gets a stack, the runtime
# takes the lock around the shared array, and the main thread joins them all.
lock = Mutex.new
seen = []
threads = (1..4).map do |i|
  Thread.new(i) do |n|
    lock.synchronize { seen << n * n }
  end
end
threads.each(&:join)
p seen.sort
__END__
[1, 4, 9, 16]
