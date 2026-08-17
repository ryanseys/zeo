# A terminal the lazy pipeline does not fuse -- sum, count, min, include? --
# runs on the materialized result. Without an arm the call answered nil or
# refused to compile.
p([1, 2, 3].lazy.sum)
p([1, 2, 3].lazy.count)
p([1, 2, 3].lazy.min)
p([1, 2, 3].lazy.max)
p([1, 2, 3].lazy.include?(2))
p([1, 2, 3].lazy.include?(9))
p([1, 2, 3].lazy.minmax)
p([1, 2, 3].lazy.sort)
p([1, 2, 3].lazy.reduce(:+))
p([3, 1, 2].lazy.sort_by { |x| -x })
p([1, 2, 3].lazy.map { |x| x * 2 }.sum)
p([1, 2, 3].lazy.select { |x| x > 1 }.count)

# the fused terminals are unchanged
p([1, 2, 3].lazy.to_a)
p([1, 2, 3].lazy.first(2))
p([1, 2, 3].lazy.map { |x| x + 1 }.to_a)
p((1..Float::INFINITY).lazy.map { |x| x * 2 }.first(3))
