# Array#shuffle and #sample don't accept the `random:` keyword argument that
# lets a caller supply their own Random instance; zeo raises ArgumentError
# instead of using it as the source of randomness.
p [1, 2, 3].shuffle(random: Random.new(42))
p [1, 2, 3].sample(random: Random.new(42))
