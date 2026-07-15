require "set"

s = Set.new([1, 2, 3, 2, 1])
puts s.size
puts s.include?(2)
s.add(4)
s << 5
s.delete(3)
puts s.to_a.length
puts s.member?(3)

a = Set.new([1, 2, 3])
b = Set.new([3, 4])
puts (a | b).size
puts (a & b).to_a.length
puts (a - b).size
puts (a ^ b).size
puts a.subset?(Set.new([1, 2, 3, 9]))
puts Set.new([3, 2, 1]) == Set.new([1, 2, 3])
puts a == b

doubled = a.map { |x| x * 2 }
puts doubled.length
puts a.select { |x| x > 1 }.length
puts a.any? { |x| x > 2 }
puts a.all? { |x| x > 0 }
puts a.count
puts Set.new.empty?
