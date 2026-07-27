# Every route a String can take to a write. A String carries its own byte
# count, so an embedded NUL must not truncate the write -- and the answer must
# not depend on which spelling of the receiver or which write form was used.
# NUL is framing, not an exotic case: a WebSocket length field, a PNG header,
# any packed binary record.
require "stringio"

BIN = "AB\x00CD"
LEAD = "\x00AB"

path = "spinel_binwrite_#{Process.pid}.tmp"

# File#write, single and multi-arg
File.open(path, "wb") { |f| f.write(BIN) }
puts File.binread(path).bytes.inspect

File.open(path, "wb") { |f| p f.write(BIN, LEAD) }
puts File.binread(path).bytes.inspect

# a payload whose FIRST byte is NUL wrote nothing at all
File.open(path, "wb") { |f| p f.write(LEAD) }
puts File.binread(path).bytes.inspect

# IO#<<
File.open(path, "wb") { |f| f << BIN }
puts File.binread(path).bytes.inspect

# IO.copy_stream: the source read is a marked heap string with NULs in it
src = path + ".src"
File.binwrite(src, BIN + LEAD)
IO.copy_stream(src, path)
puts File.binread(path).bytes.inspect

# a non-String argument still stringifies
File.open(path, "wb") { |f| f.write(1234) }
puts File.binread(path).bytes.inspect

File.delete(path)
File.delete(src)
