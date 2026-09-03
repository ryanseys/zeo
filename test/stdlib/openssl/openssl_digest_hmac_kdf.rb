# OpenSSL tier 1: versions, Digest, HMAC, KDF, Random, secure compares.
# Library version STRINGS are build-specific (zeo vendors its own OpenSSL
# 3.x), so the golden prints shape checks, never the raw strings.
require "openssl"

puts "-- versions --"
p OpenSSL::VERSION
p OpenSSL::OPENSSL_VERSION.start_with?("OpenSSL 3.")
p OpenSSL::OPENSSL_LIBRARY_VERSION.start_with?("OpenSSL 3.")
p OpenSSL::OPENSSL_VERSION_NUMBER >= 0x30000000

puts "-- digest --"
d = OpenSSL::Digest.new("SHA256")
p d.class
p d.name
p d.hexdigest("abc")
d2 = OpenSSL::Digest::SHA256.new
d2 << "a"
d2.update("bc")
p d2.hexdigest
p d2.hexdigest # peek does not consume
p d2.digest.bytesize
p d2.digest_length
p d2.block_length
d2.reset
p d2.hexdigest == OpenSSL::Digest::SHA256.new.hexdigest
p OpenSSL::Digest::SHA256.hexdigest("abc")
p OpenSSL::Digest::SHA256.digest("abc").bytesize
p OpenSSL::Digest::SHA1.hexdigest("abc")
p OpenSSL::Digest::MD5.hexdigest("abc")
p OpenSSL::Digest.hexdigest("SHA1", "abc")
p OpenSSL::Digest.digest("MD5", "abc").bytesize
p OpenSSL::Digest.new("sha256").name
p OpenSSL::Digest::SHA256.new("abc").hexdigest
p [OpenSSL::Digest::MD5.new.digest_length, OpenSSL::Digest::SHA1.new.digest_length,
   OpenSSL::Digest::SHA384.new.digest_length, OpenSSL::Digest::SHA512.new.digest_length]
seeded = OpenSSL::Digest.new("SHA512", "hello ")
seeded << "world"
p seeded.hexdigest == OpenSSL::Digest::SHA512.hexdigest("hello world")
begin
  OpenSSL::Digest.new("NOPE")
rescue => e
  p e.class
  p e.class.ancestors.include?(OpenSSL::OpenSSLError)
end

puts "-- hmac --"
p OpenSSL::HMAC.hexdigest("SHA256", "key", "The quick brown fox")
p OpenSSL::HMAC.hexdigest(OpenSSL::Digest.new("SHA256"), "key", "The quick brown fox")
p OpenSSL::HMAC.digest("SHA1", "key", "data").bytesize
p OpenSSL::HMAC.base64digest("SHA256", "key", "data")
h = OpenSSL::HMAC.new("key", "SHA256")
p h.class
h << "The quick "
h.update("brown fox")
p h.hexdigest
p h.digest.bytesize
p h.to_s == h.hexdigest
h2 = OpenSSL::HMAC.new("key", OpenSSL::Digest.new("SHA256"))
h2.update("The quick brown fox")
p h2.hexdigest == h.hexdigest
p h == h2
h.reset
h.update("x")
p h.hexdigest == OpenSSL::HMAC.hexdigest("SHA256", "key", "x")
begin
  OpenSSL::HMAC.new("key", "NOPE")
rescue => e
  p e.class
end

puts "-- kdf --"
p OpenSSL::KDF.pbkdf2_hmac("password", salt: "salt", iterations: 1000,
                           length: 32, hash: "SHA256").unpack1("H*")
p OpenSSL::KDF.pbkdf2_hmac("password", salt: "salt", iterations: 1,
                           length: 20, hash: OpenSSL::Digest.new("SHA1")).unpack1("H*")
p OpenSSL::KDF.hkdf("ikm", salt: "salt", info: "info", length: 32, hash: "SHA256").unpack1("H*")
p OpenSSL::KDF.hkdf("ikm", salt: "", info: "", length: 16, hash: "SHA256").unpack1("H*")
p OpenSSL::KDF.scrypt("password", salt: "salt", N: 1024, r: 8, p: 16, length: 16).unpack1("H*")
begin
  OpenSSL::KDF.pbkdf2_hmac("password", salt: "salt", iterations: 1000, length: 32)
rescue => e
  p [e.class, e.message]
end
p OpenSSL::KDF::KDFError.superclass
p OpenSSL::PKCS5.pbkdf2_hmac("password", "salt", 1000, 32, "SHA256") ==
  OpenSSL::KDF.pbkdf2_hmac("password", salt: "salt", iterations: 1000, length: 32, hash: "SHA256")

puts "-- random + compares --"
p OpenSSL::Random.random_bytes(16).bytesize
p OpenSSL::Random.random_bytes(16).encoding
p OpenSSL.secure_compare("a" * 40, "a" * 40)
p OpenSSL.secure_compare("secret", "secreT")
p OpenSSL.fixed_length_secure_compare("abc", "abc")
begin
  OpenSSL.fixed_length_secure_compare("abc", "abcd")
rescue => e
  p [e.class, e.message]
end

puts "-- exception hierarchy --"
p OpenSSL::OpenSSLError.superclass
p OpenSSL::Digest::DigestError.superclass
p OpenSSL::BNError.superclass
p OpenSSL::Cipher::CipherError.superclass
p OpenSSL::Cipher::AuthTagError.superclass
p OpenSSL::SSL::SSLError.superclass
begin
  OpenSSL::Digest.new("NOPE")
rescue OpenSSL::OpenSSLError => e
  puts "caught as OpenSSLError"
end
__END__
-- versions --
"4.0.2"
true
true
true
-- digest --
OpenSSL::Digest
"SHA256"
"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
32
32
64
true
"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
32
"a9993e364706816aba3e25717850c26c9cd0d89d"
"900150983cd24fb0d6963f7d28e17f72"
"a9993e364706816aba3e25717850c26c9cd0d89d"
16
"SHA256"
"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
[16, 20, 48, 64]
true
OpenSSL::Digest::DigestError
true
-- hmac --
"203d1e5cedd2d18f8c5a3beff0bd9c1ebcb97097dfcb288c46b00c9227fde2c0"
"203d1e5cedd2d18f8c5a3beff0bd9c1ebcb97097dfcb288c46b00c9227fde2c0"
20
"UDH+PZicbRU3oBP6bnOdojRj/a7DtwE32Cjjas4iG9A="
OpenSSL::HMAC
"203d1e5cedd2d18f8c5a3beff0bd9c1ebcb97097dfcb288c46b00c9227fde2c0"
32
true
true
true
true
OpenSSL::Digest::DigestError
-- kdf --
"632c2812e46d4604102ba7618e9d6d7d2f8128f6266b4a03264d2a0460b7dcb3"
"0c60c80f961f0e71f3a9b524af6012062fe037a6"
"fe8f9615d2374c0d17f77d1aeaf408c2e75fe0466073d0def23c733e2f862dfd"
"d58e7c1c4394984eb3ca168ff7b709d4"
"1effd93afcf2b28964026631bf4362b0"
[ArgumentError, "missing keyword: :hash"]
OpenSSL::OpenSSLError
true
-- random + compares --
16
#<Encoding:BINARY (ASCII-8BIT)>
true
false
true
[ArgumentError, "inputs must be of equal length"]
-- exception hierarchy --
StandardError
OpenSSL::OpenSSLError
OpenSSL::OpenSSLError
OpenSSL::OpenSSLError
OpenSSL::Cipher::CipherError
OpenSSL::OpenSSLError
caught as OpenSSLError
