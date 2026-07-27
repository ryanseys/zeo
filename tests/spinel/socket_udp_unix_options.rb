# UDPSocket, the UNIX-domain pair, and the socket-option family.
require "socket"

# --- UDP ---
u = UDPSocket.new
p u.class
p u.is_a?(IPSocket)
u.bind("127.0.0.1", 0)
port = u.addr[1]
s = UDPSocket.new
s.send("ping", 0, "127.0.0.1", port)
msg, from = u.recvfrom(16)
p msg
p from[0]
p from[2]
s.close
u.close

# a connected UDP socket needs no address on send
a = UDPSocket.new
a.bind("127.0.0.1", 0)
aport = a.addr[1]
b = UDPSocket.new
b.connect("127.0.0.1", aport)
b.send("pong", 0)
p a.recv(16)
b.close
a.close

# --- UNIX domain ---
path = "spinel_ux_suite.sock"
File.delete(path) if File.exist?(path)
srv = UNIXServer.new(path)
p srv.class
p srv.is_a?(BasicSocket)
t = Thread.new { c = srv.accept; c.write("hi\n"); sleep 0.05; c.close }
us = UNIXSocket.new(path)
p us.class
p us.gets
us.close
t.join
srv.close
File.delete(path)

# --- options and constants ---
p Socket::SOL_SOCKET.class
# print no raw AF_/SOCK_ values: they differ by platform
p Socket::AF_INET.is_a?(Integer)
p Socket::SOCK_DGRAM != Socket::SOCK_STREAM
srv2 = TCPServer.new("127.0.0.1", 0)
p srv2.setsockopt(Socket::SOL_SOCKET, Socket::SO_KEEPALIVE, 1)
p srv2.getsockopt(Socket::SOL_SOCKET, Socket::SO_KEEPALIVE).int != 0
srv2.close
r = (Socket::BOGUS_XYZ rescue $!.class); p r
