require "digest"

# Digest output and internal block sizes.
p Digest::MD5.new.digest_length
p Digest::SHA1.new.digest_length
p Digest::SHA256.new.digest_length
p Digest::SHA512.new.digest_length
p Digest::SHA256.new.block_length
p Digest::SHA512.new.block_length

# == compares by raw digest (another Digest) or hexdigest (a String).
a = Digest::SHA256.new; a.update("hello")
b = Digest::SHA256.new; b.update("hello")
p(a == b)
p(a == Digest::SHA256.hexdigest("hello"))
p(a == "not the digest")

# Digest.hexencode hex-encodes bytes without hashing.
p Digest.hexencode("Hi!")
