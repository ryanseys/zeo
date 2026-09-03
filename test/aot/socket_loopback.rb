# Sockets link: the binary opens a loopback listener, connects to it and
# reads the bytes back.
require "socket"
server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]
writer = Thread.new do
  sock = TCPSocket.new("127.0.0.1", port)
  sock.write("ping\n")
  sock.close_write
  print sock.read
  sock.close
end
client = server.accept
puts client.gets.chomp
client.write("pong\n")
client.close
writer.join
server.close
__END__
ping
pong
