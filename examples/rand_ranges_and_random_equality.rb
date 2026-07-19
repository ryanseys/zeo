# Kernel#rand / Random#rand range semantics, and Random equality.

# A range argument draws a value within it (Integer range -> Integer, a Float
# endpoint -> Float). srand makes the draw deterministic, so assert membership.
srand(3)
p (1..1000).cover?(rand(1..1000))
p (0.0...1.0).cover?(rand(0.0...1.0))

# An empty or reversed range answers nil (NOT an error).
p rand(5...5)
p rand(5..3)
p rand(2.0...2.0)

# A beginless or endless range is a domain error.
def caught
  yield
rescue => e
  "#{e.class}: #{e.message}"
end
p caught { rand(1..) }
p caught { rand(..5) }

# A negative bound draws from [0, |n|).
p rand(-3) >= 0
p rand(-3) < 3

# Random#== compares seed and stream position (not identity): two fresh
# same-seed generators are equal until one draws.
a = Random.new(1)
b = Random.new(1)
p a == b
a.rand(100)
p a == b
p Random.new(1) == Random.new(2)
p Random.new_seed.class
