# #feed sets the value the paused y.yield returns on the next #next;
# feeding twice before a #next raises TypeError; #feed answers nil.

g = Enumerator.new { |y| got = y.yield(10); y << (got * 2) }
p g.next
g.feed(5)
p g.next
k = Enumerator.new { |y| y.yield(1) }
k.next
p k.feed(:x)
m = Enumerator.new { |y| y.yield(1); y.yield(2) }
m.next
m.feed(:a)
begin
  m.feed(:b)
rescue => e
  p e.message
end
__END__
10
10
nil
"feed value already set"
