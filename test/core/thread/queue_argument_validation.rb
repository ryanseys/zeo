# Queue-family argument surfaces: `Queue#pop(true)` takes the non_block
# flag and raises ThreadError ("queue empty") on an empty queue; zeo's row
# takes no arguments at all. `SizedQueue.new(0)` raises ArgumentError
# ("queue size must be positive"); zeo mints the queue.
begin
  Queue.new.pop(true)
rescue ThreadError => e
  puts "#{e.class}: #{e.message}"
end
begin
  SizedQueue.new(0)
  puts "sized queue minted"
rescue ArgumentError => e
  puts e.message
end
__END__
ThreadError: queue empty
queue size must be positive
