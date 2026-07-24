# Variadic FFI and Ruby-Proc callbacks, AOT-compiled through libffi: a
# `[.., :varargs]` function builds its call interface at runtime, and a
# `callback` type turns a Ruby Proc into a C function pointer.
require "ffi"

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :snprintf, [:pointer, :size_t, :string, :varargs], :int
  callback :cmp, [:pointer, :pointer], :int
  attach_function :qsort, [:pointer, :size_t, :size_t, :cmp], :void
end

# Variadic: format mixed int/string/double varargs into a buffer.
buf = FFI::MemoryPointer.new(:char, 64)
n = C.snprintf(buf, 64, "%d items, %s, %.2f done", :int, 3, :string, "ok", :double, 0.75)
puts n
puts buf.read_string

# Callback: a Ruby Proc sorts a C array in place via qsort.
arr = FFI::MemoryPointer.new(:int32, 6)
arr.write_array_of_int32([5, 2, 8, 1, 9, 3])
C.qsort(arr, 6, 4, proc { |a, b| a.read_int32 <=> b.read_int32 })
p arr.read_array_of_int32(6)
