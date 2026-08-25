# Three socket-surface rows: `BasicSocket.do_not_reverse_lookup` (the
# class-level accessor), `Socket.sockaddr_un`/`unpack_sockaddr_un`, and
# the ECONNREFUSED message shape ('Connection refused - connect(2) for
# "127.0.0.1" port N'; zeo leaks rust's io text). (Found by the
# 2026-08-24 probe sweep.)
require "socket"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message.sub(/port \d+/, 'port N')}"
end
show { BasicSocket.do_not_reverse_lookup }
show { Socket.unpack_sockaddr_un(Socket.sockaddr_un("/tmp/u.sock")) }
show do
  ghost = TCPServer.new("127.0.0.1", 0)
  port = ghost.addr[1]
  ghost.close
  TCPSocket.new("127.0.0.1", port)
end
