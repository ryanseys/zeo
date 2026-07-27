# The socket methods belong to the socket classes, not to every IO handle,
# and a socket is sync = true where a file is buffered.
require "socket"
srv = TCPServer.new("127.0.0.1", 0)
port = srv.addr[1]
t = Thread.new { c = srv.accept; c.write("hi\n"); sleep 0.05; c.close }
s = TCPSocket.new("127.0.0.1", port)
p s.sync
p s.gets
p s.addr.class
p s.peeraddr[0]
s.close
t.join
srv.close

dir = "spinel_sockkind_test_dir"
Dir.mkdir(dir) unless Dir.exist?(dir)
path = File.join(dir, "sample.txt")
File.write(path, "hello\n")
f = File.open(path)
p f.sync
r = (f.addr rescue $!.class); p r
r1 = (f.addr rescue $!.message); p r1
r2 = (f.peeraddr rescue $!.class); p r2
r3 = (f.accept rescue $!.class); p r3
f.close
File.delete(path)
Dir.rmdir(dir)
