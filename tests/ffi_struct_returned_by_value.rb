# An attach_function whose return type is `Struct.by_value`: the C function
# hands the struct back in registers/stack per the ABI, and the wrapper
# copies it into a fresh ruby-owned MemoryPointer viewed through the struct's
# own class. libc's div() is the canonical case, oracle-verified.
require "ffi"

module M
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  class DivT < FFI::Struct
    layout :quot, :int, :rem, :int
  end

  attach_function :div, [:int, :int], DivT.by_value
end

r = M.div(7, 3)
puts r.class
puts r[:quot]
puts r[:rem]
r2 = M.div(-9, 4)
puts r2[:quot]
puts r2[:rem]
puts r.is_a?(FFI::Struct)
