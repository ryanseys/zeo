primes = (2..Float::INFINITY).lazy.select { |n| (2..Math.sqrt(n)).none? { |d| n % d == 0 } }
p primes.first(5)
