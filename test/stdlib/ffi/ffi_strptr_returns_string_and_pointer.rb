# `:strptr` is the gem's two-for-one return: the char* decoded as a String
# AND the raw Pointer beside it, so the caller can still free the buffer.
# strerror's ENOENT message spells the same on macOS and glibc.
require "ffi"

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strerror, [:int], :strptr
end

s, p = C.strerror(2)
puts s
puts s.class
puts p.class
puts p.null?
__END__
No such file or directory
String
FFI::Pointer
false
