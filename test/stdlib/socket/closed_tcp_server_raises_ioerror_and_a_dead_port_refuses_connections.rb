# Every operation on a closed server raises IOError "closed stream"
# (close itself is idempotent), and connecting to the freed port
# raises Errno::ECONNREFUSED.

require "socket"
server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]
p server.closed?
server.close
server.close
p server.closed?
begin
  server.accept
rescue IOError => e
  puts "accept: #{e.message}"
end
begin
  server.addr
rescue IOError => e
  puts "addr: #{e.message}"
end
begin
  TCPSocket.new("127.0.0.1", port)
rescue Errno::ECONNREFUSED
  puts "refused"
end
__END__
false
true
accept: closed stream
addr: closed stream
refused
