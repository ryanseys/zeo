# A lazy select whose block runs a nested none? answers its first five.
primes = (2..Float::INFINITY).lazy.select { |n| (2..Math.sqrt(n)).none? { |d| n % d == 0 } }
p primes.first(5)
__END__
[2, 3, 5, 7, 11]
