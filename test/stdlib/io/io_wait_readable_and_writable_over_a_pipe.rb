# `require "io/wait"` is a native no-op feature (like io/console): IO gains
# `#wait_readable`/`#wait_writable`, real `poll(2)` over the fd. A pipe with
# a byte buffered is readable; its write end is writable. net/protocol's
# BufferedIO drives its read/write timeouts through exactly these.

require "io/wait"
r, w = IO.pipe
w.write("x")
puts(r.wait_readable(1) ? "readable" : "timeout")
puts(w.wait_writable(1) ? "writable" : "timeout")
__END__
readable
writable
