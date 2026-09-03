# UNIXServer / UNIXSocket over a filesystem socket. The server binds+listens on
# a path, a client connects, and #accept mints a connected UNIXSocket; bytes flow
# both ways over the IO surface, and #addr/#path report the AF_UNIX endpoints.
# The path embeds the pid so parallel runs don't collide; it's never printed
# (only compared), so the golden stays deterministic.
require "socket"

path = "/tmp/zeo_unix_test_#{Process.pid}.sock"
File.unlink(path) if File.exist?(path)

server = UNIXServer.new(path)
p server.addr[0]
p server.addr[1] == path

client = UNIXSocket.new(path)
conn = server.accept

client.write("unix-hello")
p conn.recv(10)

conn.write("reply!")
p client.read(6)

p conn.peeraddr[0]
p client.path == path

client.close
conn.close
server.close
File.unlink(path)
p :done
__END__
"AF_UNIX"
true
"unix-hello"
"reply!"
"AF_UNIX"
false
:done
