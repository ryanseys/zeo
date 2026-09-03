# FFI links libffi and dlopen into the binary: the program calls libc through
# a run-time signature.
require "ffi"

module Libc
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strlen, [:string], :size_t
  attach_function :abs, [:int], :int
end

puts Libc.strlen("zeo compiles ruby")
puts Libc.abs(-42)
buf = FFI::MemoryPointer.new(:char, 8)
buf.put_bytes(0, "abcdefg\0")
puts buf.read_string
__END__
17
42
abcdefg
