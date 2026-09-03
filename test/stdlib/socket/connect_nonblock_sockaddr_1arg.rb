require "socket"

def sockaddr_in(port, host)
  [Socket::AF_INET, port].pack("vn") +
    host.split(".").map(&:to_i).pack("C4") + ("\x00" * 8)
end

s1 = Socket.new(Socket::AF_INET, Socket::SOCK_STREAM, 0)
refused =
  begin
    s1.connect_nonblock(sockaddr_in(1, "127.0.0.1"))
  rescue => e
    e.class
  end
p refused
s1.close

srv = TCPServer.new("127.0.0.1", 0)
port = srv.addr[1]
t = Thread.new { srv.accept }
s2 = Socket.new(Socket::AF_INET, Socket::SOCK_STREAM, 0)
default =
  begin
    s2.connect_nonblock(sockaddr_in(port, "127.0.0.1"))
    :ok
  rescue IO::WaitWritable
    :wait_writable
  end
p default
IO.select(nil, [s2], nil, 5)
second =
  begin
    s2.connect_nonblock(sockaddr_in(port, "127.0.0.1"))
    :connected
  rescue Errno::EISCONN
    :connected
  end
p second
t.value.close rescue nil
srv.close
s2.close

srv2 = TCPServer.new("127.0.0.1", 0)
port2 = srv2.addr[1]
t2 = Thread.new { srv2.accept }
s3 = Socket.new(Socket::AF_INET, Socket::SOCK_STREAM, 0)
flag = s3.connect_nonblock(sockaddr_in(port2, "127.0.0.1"), exception: false)
p flag
IO.select(nil, [s3])
again = s3.connect_nonblock(sockaddr_in(port2, "127.0.0.1"), exception: false)
p again
t2.value.close rescue nil
srv2.close
s3.close
__END__
IO::EINPROGRESSWaitWritable
:wait_writable
:connected
:wait_writable
0
