# require "socket" activates TCPServer/TCPSocket. A single-threaded round
# trip: connect first (queues in the listen backlog), accept, then exchange
# bytes via the inherited IO surface (write/gets/read on TCPSocket < IO).

require "socket"
server = TCPServer.new("127.0.0.1", 0)
p server.addr[1] > 0
p server.class
c = TCPSocket.new("127.0.0.1", server.addr[1])
s = server.accept
p s.class
s.write "hello\n"
p c.gets
c.write "back\n"
p s.gets
s.print "tail"
s.close
p c.read
c.close
server.close
p server.closed?
__END__
true
TCPServer
TCPSocket
"hello\n"
"back\n"
"tail"
true
