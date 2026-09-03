# SecureRandom: cryptographically secure random values, built on OS entropy
# (Random.urandom) and the Random::Formatter mixin. Random output can't be
# printed verbatim, so this prints its OBSERVABLE CONTRACT -- lengths, formats,
# ranges, encoding -- which is identical under zeo and ruby.
require "securerandom"

# hex(n) -> 2n lowercase hex chars; default n is 16.
puts SecureRandom.hex(8).length
puts SecureRandom.hex.length
puts(SecureRandom.hex(8) =~ /\A\h{16}\z/ ? "hex_ok" : "hex_BAD")

# base64 / urlsafe_base64. n=5 isn't a multiple of 3, so a padded encoding
# actually carries a trailing '=' -- default strips it, `true` keeps it.
puts SecureRandom.base64(6).length
puts(SecureRandom.urlsafe_base64(5) =~ /\A[A-Za-z0-9\-_]+\z/ ? "urlsafe_ok" : "urlsafe_BAD")
puts SecureRandom.urlsafe_base64(5).include?("=")
puts SecureRandom.urlsafe_base64(5, true).include?("=")

# uuid -> a valid RFC 9562 version-4 UUID.
puts(SecureRandom.uuid =~ /\A\h{8}-\h{4}-4\h{3}-[89ab]\h{3}-\h{12}\z/ ? "uuid_ok" : "uuid_BAD")

# random_bytes -> a BINARY (ASCII-8BIT) string of the requested size.
puts SecureRandom.random_bytes(5).bytesize
puts SecureRandom.random_bytes(5).encoding.to_s

# random_number: integer in [0, n), or a float in [0.0, 1.0) with no argument.
puts((0...100).all? { SecureRandom.random_number(10) < 10 })
puts SecureRandom.random_number.class
puts((0...100).all? { r = SecureRandom.random_number; r >= 0.0 && r < 1.0 })

# alphanumeric.
puts(SecureRandom.alphanumeric(20) =~ /\A[A-Za-z0-9]{20}\z/ ? "alnum_ok" : "alnum_BAD")

# Two independent draws differ (a seeded PRNG would not).
puts(SecureRandom.hex(16) != SecureRandom.hex(16))
__END__
16
32
hex_ok
8
urlsafe_ok
false
true
uuid_ok
5
ASCII-8BIT
true
Float
true
alnum_ok
true
