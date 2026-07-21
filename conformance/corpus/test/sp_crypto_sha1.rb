# sp_crypto SHA-1 + WebSocket handshake sugar.
#
# SHA-1 is a legacy hash, only kept for the WebSocket handshake
# (RFC 6455 §1.3 explicitly requires it for Sec-WebSocket-Accept).
# The two FIPS-180 sample vectors and the RFC 6455 §1.3 example
# are deterministic, so this test stays tight.
module Crypto
  ffi_func :sp_crypto_sha1_hex,           [:str], :str
  ffi_func :sp_crypto_websocket_accept,   [:str], :str
end

# FIPS-180-4 standard vectors.
puts Crypto.sp_crypto_sha1_hex("abc")        # a9993e36...
puts Crypto.sp_crypto_sha1_hex("")           # da39a3ee...

# RFC 6455 §1.3 worked example. The handshake-key+GUID combo plus
# base64 of the SHA-1 hash is the only modern use of SHA-1 here.
puts Crypto.sp_crypto_websocket_accept("dGhlIHNhbXBsZSBub25jZQ==")
