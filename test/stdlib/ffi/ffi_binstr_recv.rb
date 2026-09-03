# Ported from spinel's :binstr FFI return mode to the real ffi gem API: a
# binary payload with embedded NULs survives a byte-count read
# (read_string(len)), where the C-string read (read_string with no length,
# strlen-based) truncates at the first NUL -- fatal for binary protocols
# (WebSocket frames carry 0x00). A plain strlen view reports length 1.
require "ffi"

buf = FFI::MemoryPointer.new(16)
buf.put_bytes(0, "a\0b\0c")
s = buf.read_string(5)
puts s.length
puts s.bytesize
puts s.bytes.inspect
# The strlen-truncated view stops at the first embedded NUL.
puts buf.read_string.length
__END__
5
5
[97, 0, 98, 0, 99]
1
