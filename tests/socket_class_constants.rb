# The socket classes are constants once `require "socket"` runs, and a class
# the compiler recognizes but has not implemented reports the missing METHOD
# rather than a missing constant.
require "socket"
p TCPServer
p TCPSocket
p UDPSocket
p BasicSocket
p Socket
p TCPServer.name
p TCPServer.to_s

r2 = (IPSocket.getaddress("127.0.0.1") rescue $!.class); p r2
# a NAME resolves too, but to whichever family this host prefers
p ["127.0.0.1", "::1"].include?(IPSocket.getaddress("localhost"))
r3 = (TCPServer.bogus_xyz rescue $!.class); p r3
r6 = (TCPServer.bogus_xyz rescue $!.message); p r6

srv = TCPServer.new("127.0.0.1", 0)
p srv.class
srv.close

# a genuinely undefined constant still reports the constant
r4 = (Bogus123Xyz.new rescue $!.class); p r4
r5 = (Bogus123Xyz.new rescue $!.message); p r5
