# Every Errno::* is a SystemCallError, in `rescue` as well as in #is_a?.
begin
  File.open("/nonexistent_spinel_xyz")
rescue SystemCallError
  p "caught SystemCallError"
rescue => e
  p ["other", e.class]
end
begin
  File.open("/nonexistent_spinel_xyz")
rescue => e
  p e.class
  p e.is_a?(SystemCallError)
  p e.is_a?(StandardError)
  p e.is_a?(Errno::ENOENT)
  p e.is_a?(TypeError)
end
begin
  File.open("/nonexistent_spinel_xyz")
rescue StandardError
  p "caught StandardError"
end

require "socket"
r = (TCPSocket.new("127.0.0.1", 1) rescue $!.class); p r
begin
  TCPSocket.new("127.0.0.1", 1)
rescue SystemCallError
  p "socket error is a SystemCallError"
end

# a non-Errno error must NOT be swept up by SystemCallError
begin
  raise TypeError, "x"
rescue SystemCallError
  p "wrong"
rescue TypeError
  p "TypeError stays a TypeError"
end
