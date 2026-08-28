# Three SP_BUILTIN_* cls_ids named two kinds each, because they are declared in
# two headers that cannot see one another: Addrinfo shared STRBUF's -40 and so
# answered String, Socket::Option shared OpenStruct's -41, and Enumerator
# shared FOREIGN_PTR's -25 -- the one the collector deliberately does NOT
# trace, so a boxed Enumerator was collected out from under itself (#4158).
require "socket"

ai = Addrinfo.tcp("127.0.0.1", 8080)
p [ai][0].class

s = Socket.new(Socket::AF_INET, Socket::SOCK_STREAM, 0)
p [s.getsockopt(Socket::SOL_SOCKET, Socket::SO_TYPE)][0].class
s.close

held = [[1, 2, 3].each]
GC.start
p held[0].class
p held[0].next
p held[0].next
