require "socket"
server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]
t = Thread.new do
  client = server.accept
  req = client.gets
  client.write "HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok"
  client.close
  req
end
sock = TCPSocket.new("127.0.0.1", port)
sock.write "GET / HTTP/1.0\r\n\r\n"
p sock.gets
body = sock.read
p body.include?("ok")
sock.close
p t.value
server.close
