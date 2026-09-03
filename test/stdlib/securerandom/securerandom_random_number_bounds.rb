require "securerandom"
puts SecureRandom.random_number(10).class
puts (0...200).all? { SecureRandom.random_number(10) < 10 }
puts (0...200).all? { n = SecureRandom.random_number(1); n == 0 }   # [0,1) integer => always 0
puts SecureRandom.random_number.class                              # no arg => Float in [0,1)
puts (0.0...1.0).include?(SecureRandom.random_number)
puts SecureRandom.random_number(1.5).class                         # positive Float => Float
# Range form
r = SecureRandom.random_number(5..9)
puts (5..9).include?(r)
__END__
Integer
true
true
Float
true
Float
true
