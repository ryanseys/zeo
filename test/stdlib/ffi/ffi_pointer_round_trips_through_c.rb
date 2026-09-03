# FFI `:pointer` marshaling: a `MemoryPointer` passed to a C
# function as its raw address, and a C `char *` return wrapped back as an
# `FFI::Pointer`. `strcpy(buf, "hello")` fills the buffer and returns it;
# `strlen(buf)` reads it back through the pointer. Matches CRuby+ffi.

require "ffi"
module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strcpy, [:pointer, :string], :pointer
  attach_function :strlen, [:pointer], :ulong
end
buf = FFI::MemoryPointer.new(:char, 32)
ret = C.strcpy(buf, "hello")
puts buf.read_string
puts C.strlen(buf)
puts ret.read_string
__END__
hello
5
hello
