# Pointer/nil equality through the real ffi gem API (ported from spinel's
# ffi_read_ptr): a live malloc result is not nil-equal, and a NULL pointer
# (read out of a zero-filled MemoryPointer) is.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :malloc, [:size_t], :pointer
  attach_function :free,   [:pointer], :void
end

p = LibC.malloc(64)
if p == nil
  puts "got_nil"
else
  puts "non_nil"
end
LibC.free(p)

# A MemoryPointer is zero-filled, so reading 8 bytes at offset 0 as a pointer
# gives a deterministic NULL -- handy for verifying that `ptr == nil`
# actually compares to NULL.
zero_ptr = FFI::MemoryPointer.new(16).read_pointer
if zero_ptr == nil
  puts "zero_is_nil"
else
  puts "zero_non_nil"
end
__END__
non_nil
zero_is_nil
