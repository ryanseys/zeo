# A lambda defined INSIDE an escaping block (a stored Proc) -- previously
# rejected at compile time, now composes through the capture analysis.

# Each closure in `makers` builds and immediately calls a lambda that closes
# over the closure's own parameter.
makers = [1, 2, 3].map do |n|
  -> { n * 10 }
end
p makers.map(&:call)          # [10, 20, 30]

# A lambda nested inside a stored block that closes over an enclosing-method
# local (a shared cell), mutated between calls.
def build
  total = 0
  adder = ->(x) { total += x }
  [adder, -> { total }]
end
add, get = build
add.call(5)
add.call(7)
p get.call                    # 12

# A lambda returned from within an iterator block, capturing the block param.
squares = (1..3).map { |i| -> { i * i } }
p squares.map(&:call)         # [1, 4, 9]
