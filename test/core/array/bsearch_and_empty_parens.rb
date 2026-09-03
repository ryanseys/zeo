# Array#bsearch / Range#bsearch support both CRuby modes, selected by the
# block's return type. Find-minimum mode (a boolean/nil result) answers the
# first element for which the block is true; find-any mode (a Numeric
# comparator result) treats 0 as a hit, a negative result searches the lower
# half and a positive one the upper half, answering nil when nothing matches.
nums = [0, 4, 7, 10, 12]
p nums.bsearch { |x| x >= 7 }          # find-minimum
p nums.bsearch { |x| 10 <=> x }        # find-any hit
p nums.bsearch_index { |x| 12 <=> x }
p(nums.bsearch { |x| 3 <=> x })        # find-any miss -> nil
p [1, 2, 3].bsearch { |x| 1 - x }
p((1..1000).bsearch { |x| x >= 512 })
p((1..1000).bsearch { |x| 777 <=> x })

# `()` is nil: falsy as a condition, a nil value in expression position.
p(())
p((() && true))
p((true || ()))
count = 0
while () ; count += 1 ; end
p count

# Array#concat copies every source before appending, so a self-aliasing
# concat terminates instead of feeding its own growth.
a = [1, 2]
a.concat(a, a)
p a
__END__
7
10
4
nil
1
512
777
nil
nil
true
0
[1, 2, 1, 2, 1, 2]
