# IO#readpartial returns as soon as ANY bytes are available. It used to be one
# fread of the full length, which is a different operation: fread keeps calling
# read(2) until it has the whole request or hits EOF. On a socket that waits for
# the peer to send that much or close, so the read-then-write shape every HTTP
# server has deadlocks -- and where libc returns short instead of blocking, the
# response was written to a connection the peer had already closed.
require "socket"

server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]

t = Thread.new do
  c = server.accept
  req = c.readpartial(4096)          # far more than the peer will send
  c.write("HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok")
  c.close
  req.bytesize
end

s = TCPSocket.new("127.0.0.1", port)
s.write("GET / HTTP/1.0\r\n\r\n")
body = s.read
s.close
puts t.value
puts body.to_s.bytesize
puts body.to_s.include?("ok")
server.close

# stdio's buffer comes first: a #gets before a #readpartial must not lose the
# bytes stdio has already pulled off the socket.
srv2 = TCPServer.new("127.0.0.1", 0)
p2 = srv2.addr[1]
t2 = Thread.new do
  c = srv2.accept
  line = c.gets                      # pulls the whole payload into the buffer
  rest = c.readpartial(64)           # must come from the buffer, not read(2)
  c.close
  "#{line.strip}|#{rest}"
end
c2 = TCPSocket.new("127.0.0.1", p2)
c2.write("first\r\nsecond")
c2.close
puts t2.value
srv2.close
