# Deterministic crypto vectors through Ruby's own primitives: HMAC-SHA256
# built from Digest::SHA256 (RFC 2104
# construction), Base64URL via the base64 stdlib, PBKDF2 with one iteration
# (HMAC(password, salt || 0x00000001)), and core Random bytes for the random
# leg.
#
#   - HMAC-SHA256 against RFC 4231's "Jefe" test case; the hex vector is
#     5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843.
#   - Base64URL round-trip on a 5-byte input (5 % 3 == 2, so the tail emits
#     3 chars and no padding).
#   - PBKDF2-HMAC-SHA256 with a single iteration.
#   - Random b64url returns the right length for the requested byte count
#     and doesn't compare-equal across calls.
require "digest"
require "base64"

def hmac_sha256(key, msg)
  block = 64
  key = Digest::SHA256.digest(key) if key.bytesize > block
  key = key + "\0" * (block - key.bytesize)
  ipad = key.bytes.map { |b| b ^ 0x36 }.pack("C*")
  opad = key.bytes.map { |b| b ^ 0x5c }.pack("C*")
  Digest::SHA256.digest(opad + Digest::SHA256.digest(ipad + msg))
end

def b64url(bytes)
  Base64.urlsafe_encode64(bytes).delete("=")
end

key = "Jefe"
msg = "what do ya want for nothing?"
puts hmac_sha256(key, msg).unpack1("H*")

# Base64URL round-trip; should print "aGVsbG8" then "hello".
enc = b64url("hello")
puts enc
puts Base64.urlsafe_decode64(enc)

# PBKDF2 with iters=1 is HMAC(password, salt||0x00000001). The expected
# 43-char b64url is a stable property of the salt+password+1 input.
puts b64url(hmac_sha256("password", "salt" + [1].pack("N")))

# Random: two calls of the same size must differ (probability of collision
# on 16 bytes is 2^-128). Print the length and the inequality result so the
# .expected stays deterministic.
rng = Random.new
r1 = b64url(rng.bytes(16))
r2 = b64url(rng.bytes(16))
puts r1.length
puts(r1 == r2 ? "same" : "diff")
__END__
5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
aGVsbG8
hello
Eg-2z_z4syxD5yJSVsT4N6hlSMkszDVICAWYfLcL4Xs
22
diff
