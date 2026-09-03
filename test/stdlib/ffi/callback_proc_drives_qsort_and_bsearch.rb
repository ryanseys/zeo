require "ffi"
module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  callback :cmp, [:pointer, :pointer], :int
  attach_function :qsort,   [:pointer, :size_t, :size_t, :cmp], :void
  attach_function :bsearch, [:pointer, :pointer, :size_t, :size_t, :cmp], :pointer
end
cmp = proc { |a, b| a.read_int64 <=> b.read_int64 }
arr = FFI::MemoryPointer.new(:int64, 5)
arr.write_array_of_int64([9, 3, 7, 1, 5])
L.qsort(arr, 5, 8, cmp)
p arr.read_array_of_int64(5)
arr2 = FFI::MemoryPointer.new(:int64, 4)
arr2.write_array_of_int64([10, 20, 30, 40])
L.qsort(arr2, 4, 8, proc { |a, b| b.read_int64 <=> a.read_int64 })
p arr2.read_array_of_int64(4)
key = FFI::MemoryPointer.new(:int64, 1)
key.write_int64(7)
hit = L.bsearch(key, arr, 5, 8, cmp)
puts(hit == nil ? "miss" : hit.read_int64)
key.write_int64(4)
puts(L.bsearch(key, arr, 5, 8, cmp) == nil ? "miss" : "hit")
__END__
[1, 3, 5, 7, 9]
[40, 30, 20, 10]
7
miss
