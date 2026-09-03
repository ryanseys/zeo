# FFI `callback :tag, [args], ret` declares a callback TYPE. A C callback is a
# function pointer, so the tag resolves to `:pointer` and is usable anywhere a
# pointer is -- here, a NULL passed straight back through. Passing a Ruby Proc
# as a callback argument needs a runtime C-call builder and is still a gap.

require "ffi"
module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  callback :cmp, [:pointer, :pointer], :int
  attach_function :abs, [:int], :int
  attach_function :my_len, :strlen, [:string], :ulong
end
p L.abs(-5)
p L.my_len("hello")
__END__
5
5
