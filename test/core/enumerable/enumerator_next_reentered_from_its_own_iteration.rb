# An Enumerator drives its source on a fiber, so `next` from inside that
# source would resume the running fiber, and `next` from another thread would
# resume a fiber it does not own. Both are FiberError, and neither leaves the
# enumerator unusable: the reentered one restarts, the cross-thread one keeps
# its place.
inner = nil
inner = Enumerator.new do |y|
  y << 1
  y << inner.next
end

begin
  puts inner.next
  puts inner.next
rescue FiberError => e
  puts "#{e.class}: #{e.message}"
end
puts inner.rewind.next

shared = Enumerator.new do |y|
  y << 10
  y << 20
end
puts shared.next
Thread.new do
  shared.next
rescue FiberError => e
  puts "#{e.class}: #{e.message}"
end.join
puts shared.next
__END__
1
FiberError: attempt to resume the current fiber
1
10
FiberError: fiber called across threads
20
