# The non-blocking readiness family: the IO::*Wait* exceptions, the
# `exception: false` markers, and the retry loop they exist for. Everything here
# is deterministic -- no cross-thread timing.
require "socket"

srv = TCPServer.new("127.0.0.1", 0)
begin
  srv.accept_nonblock
rescue IO::WaitReadable => e
  p ["WaitReadable", e.class]
  p e.is_a?(Errno::EAGAIN)
  p e.is_a?(SystemCallError)
end
p srv.accept_nonblock(exception: false)
srv.close

r, w = IO.pipe
p r.read_nonblock(4, exception: false)
begin
  r.read_nonblock(4)
rescue IO::WaitReadable => e
  p e.class
end
w.write("hi")
w.flush
p r.read_nonblock(4)
p w.write_nonblock("more")

# the retry idiom, with the data already in flight so the loop terminates
w.write("second")
w.flush
buf = nil
20.times do
  begin
    buf = r.read_nonblock(16)
    break
  rescue IO::WaitReadable
    IO.select([r], nil, nil, 0.05)
  end
end
p buf

# a blocking read still works after a non-blocking one on the same handle
w.write("line\n")
w.flush
p r.gets
r.close
w.close

# a nil handle names NilClass
r2, w2 = IO.pipe
p r2.wait_priority(0).nil?
r2.close
w2.close
