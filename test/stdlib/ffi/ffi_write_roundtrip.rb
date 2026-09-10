# put_* and get_* round trips through the ffi gem: each writer stores a value
# at a byte offset
# into a buffer and the matching reader recovers it, including a pointer slot
# whose NULL reads back nil-equal.
require "ffi"

buf = FFI::MemoryPointer.new(32)

buf.put_int32(0, -12345)
puts buf.get_int32(0)

buf.put_uint32(4, 4_000_000_000)
puts buf.get_uint32(4)

# a second write through the same slot overwrites the first
buf.put_int32(0, 77)
puts buf.get_int32(0)

# ptr writer: store a pointer and read it back; an untouched (zeroed) slot
# reads back as NULL.
buf.put_pointer(8, buf)           # store the buffer's own address
puts(buf.get_pointer(8) != nil)
puts(buf.get_pointer(16) == nil)
__END__
-12345
4000000000
77
true
true
