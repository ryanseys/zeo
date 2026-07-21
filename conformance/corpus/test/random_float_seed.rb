# Random.new(Float): CRuby truncates to an integer seed. An out-of-range float
# must still yield a reproducible generator (a plain C cast would be UB).
p(Random.new(1e300).rand(1000) == Random.new(1e300).rand(1000))
p(Random.new(2.0**70).rand(1000) == Random.new(2.0**70).rand(1000))
p(Random.new(3.9).rand(1000) == Random.new(3.9).rand(1000))
p(Random.new(1e300).rand(1000) != Random.new(2e300).rand(1000))
p(Random.new(3.9).rand(100) >= 0)
