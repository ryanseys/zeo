r = (TCPServer.new("127.0.0.1", 0) rescue $!.class)
p r
r2 = (TCPSocket.new("127.0.0.1", 1) rescue $!.class)
p r2
