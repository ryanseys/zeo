require "ffi"
module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  callback :cmp, [:pointer, :pointer], :int
  attach_function :qsort, [:pointer, :size_t, :size_t, :cmp], :void
end
arr = FFI::MemoryPointer.new(:int32, 3)
arr.write_array_of_int32([3, 1, 2])
begin
  L.qsort(arr, 3, 4, proc { |a, b| raise "boom from callback" })
rescue => e
  puts "rescued: #{e.message}"
end
__END__
rescued: boom from callback
