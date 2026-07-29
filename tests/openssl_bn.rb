# OpenSSL::BN over the vendored libcrypto BIGNUM. BN#inspect is the plain
# object form (address-bearing), so the golden prints values through
# to_i/to_s, never p on a BN itself.
require "openssl"

puts "-- construction --"
b = OpenSSL::BN.new(255)
p b.class
p b.to_i
p OpenSSL::BN.new("255").to_i
p OpenSSL::BN.new("ff", 16).to_i
p OpenSSL::BN.new(-42).to_i
p OpenSSL::BN.new(b).to_i
p (OpenSSL::BN.new(2)**OpenSSL::BN.new(130)).to_i
p 255.to_bn.class
p 255.to_bn == b
p b.to_bn.equal?(b)
begin
  OpenSSL::BN.new("zz")
rescue => e
  p e.class
end

puts "-- encodings --"
p b.to_s
p b.to_s(16)
p b.to_s(2).bytesize
p OpenSSL::BN.new(65537).to_s(0).unpack1("H*")
p OpenSSL::BN.new(OpenSSL::BN.new(65537).to_s(0), 0).to_i
p OpenSSL::BN.new(OpenSSL::BN.new(65537).to_s(2), 2).to_i

puts "-- arithmetic --"
p (b + OpenSSL::BN.new(1)).to_i
p (b - 5).to_i
p (b * OpenSSL::BN.new(2)).to_i
q, r = b / OpenSSL::BN.new(4)
p [q.to_i, r.to_i]
p (b % OpenSSL::BN.new(7)).to_i
p (OpenSSL::BN.new(3)**OpenSSL::BN.new(4)).to_i
p (b << 4).to_i
p (b >> 4).to_i
p b.sqr.to_i
p b.mod_exp(OpenSSL::BN.new(3), OpenSSL::BN.new(100)).to_i
p b.mod_add(OpenSSL::BN.new(10), OpenSSL::BN.new(100)).to_i
p b.mod_sub(OpenSSL::BN.new(10), OpenSSL::BN.new(100)).to_i
p b.mod_mul(OpenSSL::BN.new(10), OpenSSL::BN.new(100)).to_i
p b.mod_inverse(OpenSSL::BN.new(7)).to_i
p b.gcd(OpenSSL::BN.new(100)).to_i
begin
  OpenSSL::BN.new(10).mod_inverse(OpenSSL::BN.new(2))
rescue => e
  p e.class
end
p 5 + OpenSSL::BN.new(10)

puts "-- predicates + comparison --"
p b.num_bits
p b.num_bytes
p b.zero?
p b.one?
p b.odd?
p OpenSSL::BN.new(0).zero?
p OpenSSL::BN.new(1).one?
p b == OpenSSL::BN.new(255)
p b == 255
p (b <=> OpenSSL::BN.new(300))
p b.cmp(OpenSSL::BN.new(300))
p b.ucmp(OpenSSL::BN.new(-300))
p b < OpenSSL::BN.new(300)   # Comparable, via the gem's Ruby half
p OpenSSL::BN.new(97).prime?
p OpenSSL::BN.new(100).prime?
p b.bit_set?(0)
p b.bit_set?(8)
c = b.dup
c.set_bit!(8)
p c.to_i
p b.to_i
p OpenSSL::BN.new(255).hash == OpenSSL::BN.new(255).hash

puts "-- randomness contracts --"
p OpenSSL::BN.rand(64).num_bits <= 64
p OpenSSL::BN.rand_range(OpenSSL::BN.new(100)) < 100
p OpenSSL::BN.generate_prime(32).prime?
