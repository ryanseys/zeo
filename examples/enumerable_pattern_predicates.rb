# any?/all?/none?/one? accept a pattern argument, matched with === (case
# equality) against each element -- a class, range, regexp, or any === value.
nums = [1, 2, 3, 4]
p nums.any?(Integer)
p nums.all?(Integer)
p nums.none?(String)
p nums.one?(3)
p nums.any?(10..20)
p nums.all?(0..)

words = %w[apple banana cherry]
p words.all?(/a/)
p words.any?(/z/)
p words.count(/an/)

# Hash yields [key, value] pairs, matched against an array pattern.
h = { a: 1, b: 2 }
p h.any?([:a, 1])
p h.none?([:c, 3])
