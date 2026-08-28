# Socket.sockaddr_in and Addrinfo#to_sockaddr: the packed sockaddr String the
# 1-argument connect_nonblock takes. Without them a caller had to build the
# bytes by hand (`[Socket::AF_INET, port].pack('vn') + ...`), which only
# happens to work for AF_INET on a little-endian host (#4137, after #4135).
require "socket"

sa = Socket.sockaddr_in(4567, "127.0.0.1")
p sa.bytesize
p Socket.unpack_sockaddr_in(sa)

# pack_sockaddr_in is CRuby's documented alias, not a second implementation.
p Socket.pack_sockaddr_in(4567, "127.0.0.1") == sa

# An already-resolved endpoint packs to the same bytes -- which is the point:
# the caller does not pay for the name lookup twice.
ai = Addrinfo.tcp("127.0.0.1", 4567)
p ai.to_sockaddr == sa
p Socket.unpack_sockaddr_in(ai.to_sockaddr)

# A zero port and a zero octet both put NUL inside the string, so the result
# has to be a byte string rather than a C string.
z = Socket.sockaddr_in(0, "0.0.0.0")
p z.bytesize
p Socket.unpack_sockaddr_in(z)

# IPv6 packs to the longer sockaddr_in6, and unpacks back.
v6 = Socket.sockaddr_in(80, "::1")
p v6.bytesize
p Socket.unpack_sockaddr_in(v6)

# The round trip through a real connection, which is what the form is for.
server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]
addr = Addrinfo.tcp("127.0.0.1", port)
sock = Socket.new(addr.afamily, Socket::SOCK_STREAM, 0)
begin
  sock.connect_nonblock(addr.to_sockaddr)
rescue IO::WaitWritable
  IO.select(nil, [sock])
  begin
    sock.connect_nonblock(addr.to_sockaddr)
  rescue Errno::EISCONN
  end
end
peer = server.accept
sock.write("ping")
p peer.readpartial(4)
p Socket.unpack_sockaddr_in(addr.to_sockaddr)[0] == port
sock.close
peer.close
server.close
