# Addrinfo -- a resolved socket address value. The tcp/udp/ip/unix constructors
# fix the family, socktype, and protocol; the accessors and #inspect read them
# back. Oracle-verified against ruby 4.0.6 (Darwin family numbers: AF_INET 2,
# AF_INET6 30, AF_UNIX 1).
require "socket"

t = Addrinfo.tcp("127.0.0.1", 80)
p t.afamily == Socket::AF_INET
p t.pfamily == Socket::PF_INET
p t.socktype == Socket::SOCK_STREAM
p t.protocol == Socket::IPPROTO_TCP
p t.ip?
p t.ipv4?
p t.ipv6?
p t.unix?
p t.ip_address
p t.ip_port
p t.inspect

u = Addrinfo.udp("127.0.0.1", 53)
p u.socktype == Socket::SOCK_DGRAM
p u.protocol == Socket::IPPROTO_UDP
p u.inspect

six = Addrinfo.tcp("::1", 443)
p six.afamily == Socket::AF_INET6
p six.ipv6?
p six.ip_address
p six.inspect

bare = Addrinfo.ip("192.168.1.1")
p bare.ip_port
p bare.inspect

ux = Addrinfo.unix("/tmp/app.sock")
p ux.afamily == Socket::AF_UNIX
p ux.unix?
p ux.unix_path
p ux.inspect

# to_sockaddr round-trips through Socket.unpack_sockaddr_in.
packed = Addrinfo.tcp("127.0.0.1", 8080).to_sockaddr
p Socket.unpack_sockaddr_in(packed)
