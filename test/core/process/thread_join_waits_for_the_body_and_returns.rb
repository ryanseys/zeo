# Deterministic by construction: `join` blocks until the spawned thread's
# body has run to completion, so "in thread" always prints before "after
# join". (Real ruby agrees on this shape's ordering -- oracle-verified.)

t = Thread.new do
  puts "in thread"
end
t.join
puts "after join"
__END__
in thread
after join
