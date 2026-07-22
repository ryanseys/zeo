# Ported from spinel's ffi_buffer/ffi_read_* readers to the real ffi gem API:
# an FFI::MemoryPointer allocation is zero-filled, so reading it back through
# every reader kind at every offset proves the storage + read-pointer
# arithmetic line up.
require "ffi"

buf = FFI::MemoryPointer.new(16)
puts buf.get_uint32(0)
puts buf.get_uint32(4)
puts buf.get_uint32(8)
puts buf.get_int32(0)

# A second buffer is a distinct allocation; reading from one doesn't
# accidentally see the other's bytes.
other = FFI::MemoryPointer.new(16)
puts other.get_uint32(0)
