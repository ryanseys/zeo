require "socket"
server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]
p port > 0
p server.class
c = TCPSocket.new("127.0.0.1", port)
s = server.accept
p s.class, c.class
s.write "hello\n"
p c.gets
c.puts "back"
p s.gets
s.print "no-newline"
s.close
p c.read
c.close
server.close
p :done
