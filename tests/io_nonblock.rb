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
