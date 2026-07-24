# FFI::MemoryPointer / FFI::Pointer and :pointer marshaling (#204 follow-on):
# an owned heap buffer with typed accessors, passed to and returned from C.
require "ffi"

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strcpy, [:pointer, :string], :pointer
  attach_function :strlen, [:pointer], :ulong
end

buf = FFI::MemoryPointer.new(:char, 32)
ret = C.strcpy(buf, "hello, ffi")
puts buf.read_string
puts C.strlen(buf)
puts ret.read_string

nums = FFI::MemoryPointer.new(:int, 3)
nums.write_array_of_int([10, 20, 30])
puts nums.read_array_of_int(3).inspect
puts (nums + 4).read_int
puts nums.size
