# UDPSocket over loopback. A datagram socket binds a local port, another
# connects to it and writes; recvfrom answers the payload plus the sender's
# [family, port, host, ip] array. (Data is written with IO#write rather than
# UDPSocket#send, which zeo's `send` intrinsic shadows for untyped receivers --
# see tests/gaps/socket_send.rb.) Ports are kernel-assigned, so only their shape
# is asserted.
require "socket"

recv = UDPSocket.new
recv.bind("127.0.0.1", 0)
addr = recv.addr
p addr[0]
p addr[1] > 0
p addr[2]

port = recv.addr[1]
send = UDPSocket.new
send.connect("127.0.0.1", port)
send.write("datagram")

msg, from = recv.recvfrom(16)
p msg
p from[0]
p from[3]
p from[1] > 0

recv.close
send.close
p :done
