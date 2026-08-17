$log = []
def sink(a, b, c)
  "#{a}|#{b}|#{c}"
end
def bump(n)
  $log << n
  n
end
i = 0
puts sink(bump(1), bump(2), bump(3))
p $log

# a scalar operator argument beside a real side effect still sequences
$log = []
x = 5
puts sink(x + 1, bump(9), x * 2)
p $log

# a mutating argument must still be ordered against a read of what it mutates
arr = [1, 2, 3]
def two(a, b) = "#{a}/#{b}"
puts two(arr.shift, arr.first)
p arr
