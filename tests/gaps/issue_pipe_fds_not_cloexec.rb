# Ruby marks every descriptor it hands out close-on-exec, so a spawned child
# doesn't silently inherit its parent's files -- `IO.pipe`, `File.open` and the
# socket constructors all set `FD_CLOEXEC` at creation. zeo's `IO.pipe` sets
# `O_NONBLOCK` (see `io_class_pipe`) but not `FD_CLOEXEC`, so `F_GETFD` answers
# 0 where ruby answers 1.
#
# The fix is at every fd-creating site, not just this one, which is why it is
# filed rather than ridden along with the fcntl constant table
# (`tests/fcntl_constants.rb`).
require "fcntl"

r, w = IO.pipe
p [r.fcntl(Fcntl::F_GETFD), w.fcntl(Fcntl::F_GETFD)]
r.close
w.close

f = File.open("/etc/hosts")
p f.fcntl(Fcntl::F_GETFD)
f.close
