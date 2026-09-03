require "ffi"
module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :snprintf, [:pointer, :size_t, :string, :varargs], :int
end
buf = FFI::MemoryPointer.new(:char, 64)
n = C.snprintf(buf, 64, "%d/%s/%.1f", :int, 42, :string, "hi", :double, 2.5)
puts n
puts buf.read_string
buf2 = FFI::MemoryPointer.new(:char, 16)
C.snprintf(buf2, 16, "plain")
puts buf2.read_string
__END__
9
42/hi/2.5
plain
