# UDP over IPv6, and the Socket class methods.
require "socket"

u = UDPSocket.new(Socket::AF_INET6)
u.bind("::1", 0)
p u.addr[0]
port = u.addr[1]
s = UDPSocket.new(Socket::AF_INET6)
s.send("v6", 0, "::1", port)
msg, from = u.recvfrom(16)
p msg
p from[0]
s.close
u.close

w = UDPSocket.new           # still IPv4 by default
w.bind("127.0.0.1", 0)
p w.addr[0]
w.close

r = Socket.getaddrinfo("127.0.0.1", 80)
p r.class
p r[0][0]
p r[0][1]
p r[0][2]

a, b = Socket.pair(Socket::AF_UNIX, Socket::SOCK_STREAM, 0)
p a.class
p a.is_a?(BasicSocket)
a.write("hey\n")
a.flush
p b.gets
a.close
b.close

sk = Socket.new(Socket::AF_INET, Socket::SOCK_STREAM, 0)
p sk.class
sk.close

p Socket.gethostname.is_a?(String)
