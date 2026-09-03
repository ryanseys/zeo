# The client-TLS surface that needs no network peer: an SSLSocket over an
# unconnected socket answers its configuration and forwards the
# descriptor-level family to the socket underneath, and the RFC 6125
# hostname matcher (post_connection_check's core) is exercised through
# OpenSSL::SSL.verify_certificate_identity's own rules on a live handshake
# elsewhere. A real handshake is deliberately NOT here -- the suite is
# hermetic.
require "openssl"
require "socket"

srv = TCPServer.new("127.0.0.1", 0)
port = srv.addr[1]
sock = TCPSocket.new("127.0.0.1", port)

ctx = OpenSSL::SSL::SSLContext.new
ctx.set_params(verify_mode: OpenSSL::SSL::VERIFY_NONE)
ssl = OpenSSL::SSL::SSLSocket.new(sock, ctx)

puts "-- identity --"
p ssl.class
p ssl.state
p ssl.io.equal?(sock)
p ssl.to_io.equal?(sock)
p ssl.context.equal?(ctx)

puts "-- forwarded to the socket --"
p ssl.closed?
p ssl.fileno == sock.fileno
p ssl.peeraddr[1] == port
p ssl.addr[0]
p ssl.local_address.ip_address

puts "-- session cache constants --"
C = OpenSSL::SSL::SSLContext
p [C::SESSION_CACHE_OFF, C::SESSION_CACHE_CLIENT, C::SESSION_CACHE_SERVER,
   C::SESSION_CACHE_BOTH]
p [C::SESSION_CACHE_NO_AUTO_CLEAR, C::SESSION_CACHE_NO_INTERNAL_LOOKUP,
   C::SESSION_CACHE_NO_INTERNAL_STORE, C::SESSION_CACHE_NO_INTERNAL]
ctx.session_cache_mode = C::SESSION_CACHE_CLIENT | C::SESSION_CACHE_NO_INTERNAL_STORE
p ctx.session_cache_mode

puts "-- before the handshake --"
p ssl.peer_cert
begin
  ssl.post_connection_check("example.com")
rescue => e
  p [e.class, e.message]
end

ssl.sync_close = true
ssl.close
p sock.closed?
srv.close
__END__
-- identity --
OpenSSL::SSL::SSLSocket
"PINIT"
true
true
true
-- forwarded to the socket --
false
true
true
"AF_INET"
"127.0.0.1"
-- session cache constants --
[0, 1, 2, 3]
[128, 256, 512, 768]
513
-- before the handshake --
nil
[OpenSSL::SSL::SSLError, "Peer verification enabled, but no certificate received."]
true
