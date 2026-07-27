# The socket classes sit in CRuby's chain, and a handle answers #is_a? from it.
require "socket"
# first(5) stops before IO's own mixins: Spinel's IO does not carry
# File::Constants / Enumerable yet, which is a separate gap.
p TCPServer.ancestors.first(5)
p TCPSocket.superclass
p UDPSocket.superclass
p UNIXServer.superclass
p Socket.superclass
p File.superclass
p IO.superclass

srv = TCPServer.new("127.0.0.1", 0)
p srv.is_a?(TCPServer)
p srv.is_a?(TCPSocket)
p srv.is_a?(IPSocket)
p srv.is_a?(BasicSocket)
p srv.is_a?(IO)
p srv.is_a?(Object)
p srv.is_a?(File)
p srv.is_a?(String)
p srv.kind_of?(IO)
p srv.instance_of?(TCPServer)
p srv.instance_of?(IO)
srv.close

dir = "spinel_hier_test_dir"
Dir.mkdir(dir) unless Dir.exist?(dir)
path = File.join(dir, "s.txt")
File.write(path, "x\n")
f = File.open(path)
p f.is_a?(File)
p f.is_a?(IO)
p f.is_a?(Object)
p f.is_a?(BasicSocket)
p f.instance_of?(File)
p f.instance_of?(IO)
f.close
File.delete(path)
Dir.rmdir(dir)
