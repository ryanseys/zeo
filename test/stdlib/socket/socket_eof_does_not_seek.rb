# `eof?` on a SOCKET must not seek: after a partial read, ruby answers
# false and the rest of the stream is readable. zeo's eof? probe seeks
# and raises Errno::ESPIPE -- the same root as
# `a_pipe_read_to_eof_does_not_seek.rb`, found via Net::HTTP keep-alive
# (the second request in one `Net::HTTP.start` dies on this).
require "socket"
server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]
t = Thread.new do
  c = server.accept
  c.write("abcd")
  c.close
end
client = TCPSocket.new("127.0.0.1", port)
p client.read(2)
p client.eof?
p client.read
client.close
t.join
server.close
__END__
"ab"
false
"cd"
