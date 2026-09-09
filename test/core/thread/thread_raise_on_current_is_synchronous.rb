# `Thread#raise` on the CURRENT thread raises synchronously -- the rescue
# around the call catches it. zeo defers it like a cross-thread interrupt,
# so the rescue misses and the exception escapes afterwards.
begin
  Thread.current.raise(ArgumentError, "self-raise")
  puts "not reached"
rescue ArgumentError => e
  puts "caught #{e.message}"
end
puts "after"
__END__
caught self-raise
after
