require "socket"

# A String carrying an embedded NUL must reach the peer whole: #bytesize
# reports the header length, and the write has to use it rather than
# stopping at the first NUL. The read side was already length-aware.
payload = "AB\x00CD"

server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]

t = Thread.new do
  conn = server.accept
  got = conn.read(5)
  conn.close
  got
end

sock = TCPSocket.new("127.0.0.1", port)
wrote = sock.write(payload)
sock.close

got = t.value
server.close

p payload.bytesize
p wrote
p got.bytesize
p got.bytes

# #write_nonblock sizes its operand the same way.
srv2 = TCPServer.new("127.0.0.1", 0)
port2 = srv2.addr[1]
t2 = Thread.new do
  conn = srv2.accept
  got2 = conn.read(5)
  conn.close
  got2
end
sock2 = TCPSocket.new("127.0.0.1", port2)
wrote2 = sock2.write_nonblock(payload)
sock2.close
p wrote2
p t2.value.bytesize
srv2.close
