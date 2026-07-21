module Buf
  ffi_buffer :scratch, 16
  ffi_read_u32 :u32_at_0, 0
  ffi_read_u32 :u32_at_4, 4
  ffi_read_u32 :u32_at_8, 8
  ffi_read_i32 :i32_at_0, 0
end

# `ffi_buffer` lives in BSS so it's zero-initialised at load. Reading
# the buffer back through every reader kind at every offset proves the
# storage + read-pointer arithmetic line up.
puts Buf.u32_at_0(Buf.scratch)
puts Buf.u32_at_4(Buf.scratch)
puts Buf.u32_at_8(Buf.scratch)
puts Buf.i32_at_0(Buf.scratch)

# A second buffer is laid out next to the first; each gets its own
# slot, so reading from one doesn't accidentally see the other's bytes.
module Other
  ffi_buffer :scratch, 16
  ffi_read_u32 :u32, 0
end
puts Other.u32(Other.scratch)
