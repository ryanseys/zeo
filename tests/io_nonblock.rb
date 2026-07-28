# `require "io/nonblock"` -- `IO#nonblock?`, `#nonblock=`, and the block form
# that sets the flag only for its block. CRuby's reader form requires that
# block, and its `IO.pipe` hands back fds already marked non-blocking.
require "io/nonblock"
r, w = IO.pipe
p [r.nonblock?, w.nonblock?]
r.nonblock = false
p r.nonblock?
p r.nonblock(true) { r.nonblock? }
p r.nonblock?
begin
  r.nonblock(true)
rescue LocalJumpError => e
  p e.message
end
f = File.open("/etc/hosts")
p f.nonblock?
f.close
r.close
w.close

# The flag lives on the DESCRIPTOR and changes nothing about what the ordinary
# rows mean: `gets` still blocks for its line rather than surfacing the EAGAIN
# an empty non-blocking pipe answers with.
r2, w2 = IO.pipe
reader = Thread.new { r2.gets }
sleep 0.2
w2.puts "hello"
p reader.value
r2.close
w2.close

# The write side of the same rule: 200KB overflows the kernel pipe buffer, so
# the writer waits for the reader to make room.
r3, w3 = IO.pipe
writer = Thread.new { w3.write("x" * 200_000); w3.close; :done }
p r3.read.bytesize
p writer.value
r3.close
