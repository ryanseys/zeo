# OpenSSL::SSL's offline surface: constants, context configuration, and an
# unconnected SSLSocket. A real handshake needs a network peer, so it is not
# exercised here (the suite stays hermetic).
require "openssl"
require "socket"

puts "-- constants --"
p OpenSSL::SSL::VERIFY_NONE
p OpenSSL::SSL::VERIFY_PEER
p OpenSSL::SSL::VERIFY_FAIL_IF_NO_PEER_CERT
p OpenSSL::SSL::TLS1_2_VERSION
p OpenSSL::SSL::TLS1_3_VERSION
p OpenSSL::SSL::OP_NO_COMPRESSION.is_a?(Integer)
p OpenSSL::X509::V_OK
p OpenSSL::X509::V_ERR_CERT_HAS_EXPIRED
p OpenSSL::X509::V_ERR_HOSTNAME_MISMATCH

puts "-- context defaults --"
ctx = OpenSSL::SSL::SSLContext.new
p ctx.class
p ctx.verify_mode
p ctx.verify_hostname
p ctx.ca_file
p ctx.session_cache_mode
ctx.set_params
p ctx.verify_mode
p ctx.verify_hostname
p ctx.cert_store.class

puts "-- context overrides --"
c2 = OpenSSL::SSL::SSLContext.new
c2.set_params(verify_mode: OpenSSL::SSL::VERIFY_NONE)
p c2.verify_mode
p c2.verify_hostname
c3 = OpenSSL::SSL::SSLContext.new
c3.verify_mode = OpenSSL::SSL::VERIFY_PEER
c3.verify_hostname = true
c3.ca_file = "/nonexistent-bundle.pem"
c3.min_version = OpenSSL::SSL::TLS1_2_VERSION
c3.max_version = OpenSSL::SSL::TLS1_3_VERSION
p c3.verify_mode
p c3.verify_hostname
p c3.ca_file

puts "-- store --"
st = OpenSSL::X509::Store.new
p st.class
p st.set_default_paths.equal?(st)

puts "-- unconnected socket --"
srv = TCPServer.new("127.0.0.1", 0)
port = srv.addr[1]
sock = TCPSocket.new("127.0.0.1", port)
ssl = OpenSSL::SSL::SSLSocket.new(sock, c2)
p ssl.class
p ssl.sync_close
ssl.sync_close = true
p ssl.sync_close
p ssl.hostname
ssl.hostname = "example.com"
p ssl.hostname
p ssl.state
p ssl.io.equal?(sock)
p ssl.to_io.equal?(sock)
p ssl.context.equal?(c2)
sock.close
srv.close

puts "-- errors --"
p OpenSSL::SSL::SSLError.superclass
p OpenSSL::SSL::SSLError.ancestors.include?(OpenSSL::OpenSSLError)
p OpenSSL::X509::Certificate.superclass
__END__
-- constants --
0
1
2
771
772
true
0
10
62
-- context defaults --
OpenSSL::SSL::SSLContext
0
false
nil
2
1
true
OpenSSL::X509::Store
-- context overrides --
0
true
1
true
"/nonexistent-bundle.pem"
-- store --
OpenSSL::X509::Store
false
-- unconnected socket --
OpenSSL::SSL::SSLSocket
nil
true
nil
"example.com"
"PINIT"
true
true
true
-- errors --
OpenSSL::OpenSSLError
true
Object
