# File.read / IO#read must read to EOF, not to the size the stream reports.
#
# The size was used as the length: a file that reports 0 bytes and still yields
# them -- every /proc and /sys entry, a FIFO, a character device -- came back as
# "". Silently, since a short read is indistinguishable from an empty file.
# File.readlines on the same file was correct, which is what made it look like a
# File.read problem rather than a shared one.
#
# The size is a hint for the initial buffer now, and every stream reads to EOF
# through one reader.

# an ordinary file still round-trips, in one allocation and one read
path = "spinel_read_eof_#{Process.pid}.tmp"
File.write(path, "abc\ndef\n")
p File.read(path).length
p File.read(path)
p File.readlines(path).length

# empty really is empty
File.write(path, "")
p File.read(path)
p File.read(path).length

# a file larger than any plausible initial buffer
big = "x" * 40000
File.write(path, big)
p File.read(path).length
p File.read(path) == big

# a handle read from a non-zero offset answers the remainder
f = File.open(path, "r")
f.read(10)
p f.read.length
f.close

File.delete(path)

# a pipe: not seekable, so the size is not even a hint
r, w = IO.pipe
w.write("through a pipe\n")
w.close
p r.read
r.close

# a stream whose reported size is zero but which yields bytes. /proc is the
# case that surfaced it; elsewhere the pipe above already covers the shape, so
# assert the property only where such a file exists.
status = "/proc/self/status"
if File.exist?(status)
  p File.read(status).length > 0
  p File.read(status).length == File.readlines(status).join.length
else
  p true
  p true
end
