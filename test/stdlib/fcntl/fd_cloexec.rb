# Ruby marks every descriptor it hands out close-on-exec, so a spawned child
# inherits only the stdio deliberately given to it -- `IO.pipe` and
# `File.open` both set `FD_CLOEXEC` at creation. Without it, a pipe write end
# leaking into an unrelated child holds the reader's EOF open forever (the
# popen-family deadlock `test/stdlib/open3/open3_capture.rb` exercises for real).
require "fcntl"

r, w = IO.pipe
p [r.fcntl(Fcntl::F_GETFD), w.fcntl(Fcntl::F_GETFD)]
r.close
w.close

f = File.open("/etc/hosts")
p f.fcntl(Fcntl::F_GETFD)
f.close
__END__
[1, 1]
1
