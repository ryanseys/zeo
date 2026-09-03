# The generic Socket class is implemented over libc: new/bind/listen/accept
# on a loopback pair, resolving the bound port through Socket.pack/
# unpack_sockaddr_in and reading it back via #local_address (an Addrinfo).

require "socket"
srv = Socket.new(Socket::AF_INET, Socket::SOCK_STREAM, 0)
srv.setsockopt(Socket::SOL_SOCKET, Socket::SO_REUSEADDR, 1)
srv.bind(Socket.pack_sockaddr_in(0, "127.0.0.1"))
srv.listen(1)
port, ip = Socket.unpack_sockaddr_in(srv.getsockname)
p ip
p port > 0
cli = Socket.new(:INET, :STREAM)
cli.connect(Socket.sockaddr_in(port, "127.0.0.1"))
conn, addr = srv.accept
p conn.class
p addr.class
cli.write("ping")
p conn.recv(4)
conn.close
cli.close
srv.close
__END__
"127.0.0.1"
true
Socket
Addrinfo
"ping"
