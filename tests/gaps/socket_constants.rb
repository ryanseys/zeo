# GAP -- imported from the spinel corpus at c55d9bdb.
# Socket::SO_RCVTIMEO is not defined. The socket constant surface is a
# coupled pair (zeo_abi::SOCKET_CONSTANT_NAMES + socket_const_value in
# crates/zeo-rt/src/ext/socket/socket.rs); both need the row.
#
# Socket::TCP_KEEPIDLE / TCP_KEEPINTVL / TCP_KEEPCNT were in spinel's version
# of this test. They are Linux-only -- CRuby on macOS raises NameError for all
# three -- so pinning them here would make the golden machine-specific. The
# portable TCP constant is TCP_NODELAY.
require "socket"
p(Socket::SOL_SOCKET.is_a?(Integer))
p(Socket::SO_KEEPALIVE.is_a?(Integer))
p(Socket::IPPROTO_TCP.is_a?(Integer))
p(Socket::TCP_NODELAY.is_a?(Integer))
p(Socket::SO_RCVTIMEO.is_a?(Integer))
p(Socket::SO_SNDTIMEO.is_a?(Integer))
p(Socket::IPPROTO_IPV6.is_a?(Integer))
p(Socket::MSG_PEEK.is_a?(Integer))
p(Socket::SOCK_RAW.is_a?(Integer))
p(Socket::AF_UNSPEC == 0)

srv = TCPServer.new("127.0.0.1", 0)
port = srv.addr[1]
cli = TCPSocket.new("127.0.0.1", port)
cli.setsockopt(Socket::SOL_SOCKET, Socket::SO_KEEPALIVE, 1)
p cli.getsockopt(Socket::SOL_SOCKET, Socket::SO_KEEPALIVE).int != 0
cli.close
srv.close
