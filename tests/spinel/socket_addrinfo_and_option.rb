# Addrinfo and Socket::Option.
require "socket"

a = Addrinfo.tcp("127.0.0.1", 80)
p a.class
p a.ip_address
p a.ip_port
p a.afamily == Socket::AF_INET   # the AF_* values differ by platform
p a.ipv4?
p a.ipv6?
p a.inspect

b = Addrinfo.udp("::1", 53)
p b.afamily == Socket::AF_INET6
p b.ipv6?
p b.inspect

u = Addrinfo.unix("/tmp/spinel_ai.sock")
p u.afamily == Socket::AF_UNIX
p u.unix?

srv = TCPServer.new("127.0.0.1", 0)
la = srv.local_address
p la.class
p la.ip_address
p la.ip_port == srv.addr[1]

srv.setsockopt(Socket::SOL_SOCKET, Socket::SO_KEEPALIVE, 1)
o = srv.getsockopt(Socket::SOL_SOCKET, Socket::SO_KEEPALIVE)
p o.class
p o.int != 0
p o.bool
p o.level == Socket::SOL_SOCKET
p o.optname == Socket::SO_KEEPALIVE
srv.close
