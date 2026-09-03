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

# `Socket::Constants` is a namespace of its own, holding the same names.
# `sock_define_const` (ext/socket/constants.c) defines each one TWICE -- once
# here, once on `Socket` -- so neither table is derived from the other, and the
# module is included NOWHERE: a lookup through `Socket` never reaches it.
# zeo seeded only `Socket`, so `Socket::Constants` raised NameError; celluloid-io
# aliases it (`Constants = ::Socket::Constants`) and never got past that line.
p Socket::Constants.class
p Socket::Constants.name
p Socket.include?(Socket::Constants)
p Socket::Constants.ancestors
p Socket.const_defined?(:Constants)
p Socket::Constants.const_defined?(:AF_UNIX)
p Socket::Constants::AF_INET == Socket::AF_INET
p Socket::Constants::SOCK_STREAM == Socket::SOCK_STREAM
Aliased = ::Socket::Constants
p Aliased::AF_INET6 == Socket::AF_INET6
__END__
TCPServer
TCPSocket
UDPSocket
BasicSocket
Socket
"TCPServer"
"TCPServer"
"127.0.0.1"
true
NoMethodError
"undefined method 'bogus_xyz' for class TCPServer"
TCPServer
NameError
"uninitialized constant Bogus123Xyz"
Module
"Socket::Constants"
false
[Socket::Constants]
true
true
true
true
true
