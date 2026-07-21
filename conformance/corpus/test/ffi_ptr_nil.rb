module LibC
  ffi_func :malloc, [:size_t], :ptr
  ffi_func :free,   [:ptr],    :void
end

module Buf
  ffi_buffer :scratch, 16
  # The buffer is zero-init in BSS, so reading 8 bytes at offset 0 as a
  # pointer gives a deterministic NULL — handy for verifying that
  # `ptr == nil` actually compares to NULL.
  ffi_read_ptr :first_ptr, 0
end

p = LibC.malloc(64)
if p == nil
  puts "got_nil"
else
  puts "non_nil"
end
LibC.free(p)

zero_ptr = Buf.first_ptr(Buf.scratch)
if zero_ptr == nil
  puts "zero_is_nil"
else
  puts "zero_non_nil"
end
