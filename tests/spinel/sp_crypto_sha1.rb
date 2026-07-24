# Ported from spinel's bundled sp_crypto.c SHA-1 FFI surface. Those C
# symbols are spinel-private, so the port keeps the same deterministic
# vectors through Digest::SHA1 + Base64.
#
# SHA-1 is a legacy hash, only kept for the WebSocket handshake (RFC 6455
# 1.3 explicitly requires it for Sec-WebSocket-Accept). The two FIPS-180
# sample vectors and the RFC 6455 1.3 example are deterministic, so this
# test stays tight.
require "digest"
require "base64"

# FIPS-180-4 standard vectors.
puts Digest::SHA1.hexdigest("abc")        # a9993e36...
puts Digest::SHA1.hexdigest("")           # da39a3ee...

# RFC 6455 1.3 worked example: Sec-WebSocket-Accept is the base64 of the
# SHA-1 of the handshake key + the fixed GUID.
GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
puts Base64.strict_encode64(Digest::SHA1.digest("dGhlIHNhbXBsZSBub25jZQ==" + GUID))
