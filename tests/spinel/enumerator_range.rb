# Range#each with no block is an external Enumerator (#next/#peek/#rewind/#size),
# but only when used that way: when it is the receiver of a collection method
# (.to_a/.map/.select/.sum/...) it materializes to a typed array, so those chains
# keep their fast unboxed path.

e = (1..3).each
p e.class
p e.is_a?(Enumerator)
p e.size

p e.next
p e.peek          # does not advance
p e.next
p e.next
begin
  e.next
rescue StopIteration
  puts "stopped"
end
e.rewind
p e.next

# direct chain to an enumerator method stays an enumerator
p (5..7).each.next

# loop terminates on StopIteration
e2 = (10..13).each
acc = 0
loop { acc += e2.next }
p acc
puts "after"

# collection chains still work (and stay typed/fast)
p((1..3).each.to_a)
p((1..4).each.map { |x| x * 2 })
p((1..6).each.select(&:even?))
p((1..4).each.sum)
p((1..5).each.reduce(:+))
