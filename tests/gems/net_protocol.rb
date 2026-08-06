require "net/protocol"

# net/protocol is the transport layer net/http and net/smtp are built on.
# Its BufferedIO wraps any IO, so a StringIO exercises it with no network.
require "stringio"

io = Net::BufferedIO.new(StringIO.new("HTTP/1.1 200 OK\r\nX: 1\r\n\r\nbody"))
puts io.readuntil("\r\n").chomp
puts io.readline
puts io.readline.empty?
puts io.read(4)

puts Net::ProtocolError.ancestors.include?(StandardError)
puts Net::ProtocRetryError.ancestors.include?(Net::ProtocolError)
puts Net::ReadTimeout.new.is_a?(Timeout::Error)
puts Net::OpenTimeout.new.class
