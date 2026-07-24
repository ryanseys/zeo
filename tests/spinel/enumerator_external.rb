# Array#each with no block returns an external Enumerator. #next / #peek walk a
# cursor and raise StopIteration past the end; #rewind resets it; #size reports
# the element count. Kernel#loop rescues StopIteration to terminate.

e = [10, 20, 30].each
p e.class
p e.is_a?(Enumerator)
p e.size

p e.next
p e.peek      # peek does not advance
p e.next
p e.next
begin
  e.next
rescue StopIteration
  puts "stopped"
end

e.rewind
p e.next      # back to the start

# loop terminates cleanly on StopIteration
e2 = [1, 2, 3, 4].each
total = 0
loop do
  total += e2.next
end
p total
puts "after loop"

# the yielded values are real values usable in expressions
ws = ["ab", "cde"].each
p(ws.next + "!")
p ws.next.length

# mixed (poly) array
m = [1, "two", :three].each
p m.next
p m.next
p m.next

# a value-form loop broken explicitly still returns its break value
e3 = [7, 8, 9].each
r = loop do
  v = e3.next
  break v if v == 8
end
p r
