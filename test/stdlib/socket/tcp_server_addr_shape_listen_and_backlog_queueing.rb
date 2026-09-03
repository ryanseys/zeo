# #addr is `[family, port, name, ip]` (numeric name -- CRuby's
# do_not_reverse_lookup default); #listen on a bound listener is a
# validated no-op answering 0; and two clients that connect BEFORE any
# accept queue in the listen backlog and are handed out in order.

require "socket"
server = TCPServer.new("127.0.0.1", 0)
addr = server.addr
p addr[0]
p addr[1] > 0
p addr[2]
p addr[3]
p server.listen(5)
c1 = TCPSocket.new("127.0.0.1", addr[1])
c2 = TCPSocket.new("127.0.0.1", addr[1])
c1.puts "first"
c2.puts "second"
s1 = server.accept
p s1.gets
s2 = server.accept
p s2.gets
[c1, c2, s1, s2].each(&:close)
server.close
__END__
"AF_INET"
true
"127.0.0.1"
"127.0.0.1"
0
"first\n"
"second\n"
