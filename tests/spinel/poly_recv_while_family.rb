# find_all / take_while / drop_while on a receiver only known to be a
# container at run time. select and reject had an arm; these three fell
# through to the loud NoMethodError.
rows = [[1, 2, 4, 5], { "a" => 1, "b" => 2 }, "x"]

arr = rows[0]
p arr.take_while { |x| x < 4 }
p arr.drop_while { |x| x < 4 }
p arr.find_all { |x| x > 2 }
p arr.select { |x| x > 2 }
p arr.reject { |x| x > 2 }

# find_all is not a third spelling of select: on a Hash it answers the pairs
# as an Array, where select answers a Hash.
h = rows[1]
p h.take_while { |k, n| n < 2 }
p h.drop_while { |k, n| n < 2 }
p h.find_all { |k, n| n > 1 }
p h.select { |k, n| n > 1 }

# and each answers nil through a safe-nav guard
def pick(n) = n > 0 ? [1, 2, 4, 5] : nil
[1, 0].each do |k|
  v = pick(k)
  p v&.take_while { |x| x < 4 }
  p v&.drop_while { |x| x < 4 }
  p v&.find_all { |x| x > 2 }
end
