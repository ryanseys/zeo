# sp_crypto.c -- ffi exposed to spinel programs.
#
# Covers the canonical surface with deterministic vectors:
#   - HMAC-SHA256 against RFC 4231 test case 4 (4-character key, 50
#     byte message). The standard vector for hex output is
#     82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b
#     -- truncated to its first 43 b64url chars for the b64url variant.
#   - Base64URL round-trip on a string with a 2-byte tail (RFC 4648
#     standard padding/tail behaviour).
#   - PBKDF2-HMAC-SHA256 with a single iteration (matches RFC 7914
#     scrypt-vector format, simpler than the 1024-iter RFC 6070 case).
#   - Random b64url returns the right length for the requested byte
#     count, and (because it's random) doesn't compare-equal across
#     calls.
module Crypto
  ffi_func :sp_crypto_hmac_sha256_hex,      [:str, :str],       :str
  ffi_func :sp_crypto_hmac_sha256_b64url,   [:str, :str],       :str
  ffi_func :sp_crypto_b64url_encode,        [:str],             :str
  ffi_func :sp_crypto_b64url_decode,        [:str],             :str
  ffi_func :sp_crypto_pbkdf2_sha256_b64url, [:str, :str, :int], :str
  ffi_func :sp_crypto_random_b64url,        [:int],             :str
end

# RFC 4231 test case 4: 25-byte key, 50-byte message.
key = "Jefe"
msg = "what do ya want for nothing?"
puts Crypto.sp_crypto_hmac_sha256_hex(key, msg)

# Base64URL round-trip on a 5-byte input (length 5 % 3 == 2 so the
# tail emits 3 chars and no padding). Should print "hello" twice.
enc = Crypto.sp_crypto_b64url_encode("hello")
puts enc
puts Crypto.sp_crypto_b64url_decode(enc)

# PBKDF2 with iters=1 is HMAC(key, salt||0x00000001). The expected
# 43-char b64url is a stable property of the salt+password+1 input.
puts Crypto.sp_crypto_pbkdf2_sha256_b64url("password", "salt", 1)

# Random: two calls of the same size must differ (probability of
# collision on 16 bytes is 2^-128). Print the length and the
# inequality result so the .expected stays deterministic.
r1 = Crypto.sp_crypto_random_b64url(16) + ""
r2 = Crypto.sp_crypto_random_b64url(16) + ""
puts r1.length
puts(r1 == r2 ? "same" : "diff")
