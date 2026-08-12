# `blocking: true` beside a callback argument is legal: the gem releases the
# GVL around the C body and the callback trampoline re-acquires before
# re-entering ruby (rdkafka's poll loop is the shape). qsort sorting through
# a ruby comparator, oracle-verified.
require "ffi"

module Q
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  callback :cmp_t, [:pointer, :pointer], :int
  attach_function :qsort, [:pointer, :size_t, :size_t, :cmp_t], :void, blocking: true
end

buf = FFI::MemoryPointer.new(:int32, 5)
buf.write_array_of_int32([31, 4, 15, 9, 2])
Q.qsort(buf, 5, 4, proc { |a, b| a.read_int32 <=> b.read_int32 })
p buf.read_array_of_int32(5)
