# The Array Tier A surface: set ops, mutators, sort family, flatten/
# compact/uniq, join/to_h, fetch/dig/zip/rotate/values_at, and the
# in-place filters' self-or-nil contract.

p [1, 2] + [3]
p [1, 2, 3] - [2]
p [1, 2] * 2
p [1, 2] * ","
p([1, 2, 3] & [2, 3, 4])
p([1, 2] | [2, 3])
p([1, 2, 3] <=> [1, 2, 4])
p [3, 1, 2].sort
p [3, 1, 2].sort { |a, b| b <=> a }
a = [1, 2, 3]
p a.pop
p a.shift
a.unshift(9)
a.push(8, 7)
p a
p [1, [2, [3]]].flatten
p [1, [2, [3]]].flatten(1)
p [1, nil, 2, nil].compact
p [1, 2, 2, 3, 1].uniq
p [1, 2, 3].reverse
p [[1, :a], [2, :b]].to_h
p [1, 2, 3].join
p [1, 2, 3].join("-")
p [1, 2, 3].index(2)
p [1, 2, 3].index(9)
p [1, 2, 1].rindex(1)
p [[1, [2, 3]]].dig(0, 1, 0)
p [1, 2].fetch(0)
p [1, 2].fetch(9, :fallback)
begin
  [1, 2].fetch(9)
rescue IndexError => e
  puts "IndexError"
end
p [1, 2, 3, 4].take(2)
p [1, 2, 3, 4].drop(2)
p [1, 2, 3].zip([4, 5, 6], [7, 8, 9])
p [1, 2, 3].rotate
p [1, 2, 3].rotate(2)
p [1, 2, 3, 4, 5].values_at(0, 2, 4)
p [1, 2, 3].at(-1)
p [0, 1, 2, 3, 4][1..3]
p [1, 2, 3].delete(2)
p [1, 2, 3].delete_at(0)
p [1, 2, 3].insert(1, :x)
p [1, 2].concat([3, 4])
p [1, 2, 3].fill(0)
p [1, 2, 3].clear
b = [3, 1, 2]
b.sort!
p b
c = [1, 2, 3]
c.map! { |x| x * 10 }
p c
p [1, 2, 3, 4].select! { |x| x > 2 }
p [1, 2, 3, 4].reject! { |x| x > 2 }
p [1, 2].select! { |x| x > 0 }
__END__
[1, 2, 3]
[1, 3]
[1, 2, 1, 2]
"1,2"
[2, 3]
[1, 2, 3]
-1
[1, 2, 3]
[3, 2, 1]
3
1
[9, 2, 8, 7]
[1, 2, 3]
[1, 2, [3]]
[1, 2]
[1, 2, 3]
[3, 2, 1]
{1 => :a, 2 => :b}
"123"
"1-2-3"
1
nil
2
2
1
:fallback
IndexError
[1, 2]
[3, 4]
[[1, 4, 7], [2, 5, 8], [3, 6, 9]]
[2, 3, 1]
[3, 1, 2]
[1, 3, 5]
3
[1, 2, 3]
2
1
[1, :x, 2, 3]
[1, 2, 3, 4]
[0, 0, 0]
[]
[1, 2, 3]
[10, 20, 30]
[3, 4]
[1, 2]
nil
