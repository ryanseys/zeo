# Void-returning FFI functions through the ffi gem.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :malloc, [:size_t], :pointer
  attach_function :free,   [:pointer], :void
end

# void-returning functions can be used in any expression position;
# the call evaluates and the value is dropped.
p = LibC.malloc(8)
LibC.free(p)
puts "freed"

# As an expression: the sequence evaluates to its last value.
x = (LibC.free(LibC.malloc(8)); 42)
puts x
__END__
freed
42
