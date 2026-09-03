# `eof?` on a PIPE or SOCKET peeks one byte instead of seeking.
#
# The old implementation probed end-of-file with `pos` + `seek(End)` + seek
# back, which is `ESPIPE` on anything that cannot seek -- so `eof?` raised
# for every pipe and socket, and Net::HTTP keep-alive died on the second
# request in one `start`.
#
# A peek COSTS a byte, so the byte has to go somewhere every reader looks.
# That is the whole design: it is parked in the read-ahead buffer and served
# by one funnel, so `read`, `read(n)`, `getbyte` and `each_codepoint` all see
# it. Two traps came with it, and both are rows here:
#
#   * the buffer is given back by seeking BACKWARD before an unbuffered row
#     runs -- which a non-seekable descriptor cannot do, so it keeps its
#     bytes rather than dropping them (dropping consumed the peek and handed
#     the next reader the byte AFTER it);
#   * `BasicSocket#recv` goes straight to the descriptor and cannot see the
#     buffer at all, so it REFUSES, which is what ruby does.

require "socket"

def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end

# --- A pipe ---------------------------------------------------------------
r, w = IO.pipe
w.write("abcd")
w.close
show { r.read(2) }
show { r.eof? }
show { r.read(1) }
show { r.getbyte }
show { r.read }
show { r.eof? }
r.close

# Reading a pipe to the end, which is what the seek probe used to raise on.
r2, w2 = IO.pipe
w2.write("hello")
w2.close
show { r2.read }
show { r2.eof? }
r2.close

# An empty pipe is at end as soon as the writer closes.
r3, w3 = IO.pipe
w3.close
show { r3.eof? }
show { r3.read }
r3.close

# --- A socket -------------------------------------------------------------
server = TCPServer.new("127.0.0.1", 0)
port = server.addr[1]
t = Thread.new do
  c = server.accept
  c.write("abcdef")
  c.close
end
client = TCPSocket.new("127.0.0.1", port)
show { client.read(2) }
show { client.eof? }
# `recv` cannot see the peeked byte, so it refuses rather than skipping it.
show { client.recv(2) }
show { client.read }
show { client.eof? }
client.close
t.join
server.close

# --- A regular file keeps the exact position probe ------------------------
require "tmpdir"
Dir.mktmpdir do |dir|
  path = File.join(dir, "f")
  File.write(path, "abcd")
  File.open(path) do |f|
    show { f.eof? }
    show { f.read(2) }
    show { f.eof? }
    show { f.read }
    show { f.eof? }
    show { f.pos }
  end
end
__END__
"ab"
false
"c"
100
""
true
"hello"
true
true
""
"ab"
false
IOError: recv for buffered IO
"cdef"
true
false
"ab"
false
"cd"
true
4
