# The keystone: external iteration over a method-backed enumerator --
# `next` advances a real fiber, `peek` caches without consuming, the
# classic `loop { e.next }` idiom terminates via StopIteration and
# returns the underlying `each`'s result, and a rescued StopIteration
# exposes `message`/`result`.

e = [10, 20, 30].each
r = loop do
  puts e.next
end
p r
e2 = [1].each
e2.next
begin
  e2.next
rescue StopIteration => ex
  puts "rescued: #{ex.message}"
  p ex.result
end
g = [7, 8].each
p g.peek
p g.peek
p g.next
p g.next
__END__
10
20
30
[10, 20, 30]
rescued: iteration reached an end
[1]
7
7
7
8
