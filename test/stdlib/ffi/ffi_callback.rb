# A `callback` type declares a C function-pointer type, and a Ruby proc passed
# to an argument of that type becomes a C-callable trampoline -- so Ruby code
# can be handed to qsort's comparator and bsearch's. Exercising both qsort
# (`void *base`) and bsearch (`const void *base`) keeps the original's
# coverage of both const-qualifications. (The original also registered an
# atexit callback; ruby-ffi cannot run a Ruby callback during process exit,
# so that leg is dropped in the port.)
require "ffi"

module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  callback :cmp, [:pointer, :pointer], :int
  attach_function :qsort,   [:pointer, :size_t, :size_t, :cmp], :void
  attach_function :bsearch, [:pointer, :pointer, :size_t, :size_t, :cmp], :pointer
end

CMP  = proc { |a, b| a.read_int64 <=> b.read_int64 }
RCMP = proc { |a, b| b.read_int64 <=> a.read_int64 }

arr = FFI::MemoryPointer.new(:int64, 8)
arr.write_array_of_int64([3, 1, 4, 1, 5, 9, 2, 6])
L.qsort(arr, 8, 8, CMP)
p arr.read_array_of_int64(8)                 # ascending

arr2 = FFI::MemoryPointer.new(:int64, 4)
arr2.write_array_of_int64([10, 20, 30, 40])
L.qsort(arr2, 4, 8, RCMP)
p arr2.read_array_of_int64(4)                # descending

# bsearch over the ascending array. The key is a one-element buffer holding
# the value to find.
key = FFI::MemoryPointer.new(:int64, 1)
key.write_int64(5)
hit = L.bsearch(key, arr, 8, 8, CMP)
puts hit == nil ? "miss" : hit.read_int64    # 5

key.write_int64(7)
miss = L.bsearch(key, arr, 8, 8, CMP)
puts miss == nil ? "miss" : miss.read_int64  # miss

puts "done"
__END__
[1, 1, 2, 3, 4, 5, 6, 9]
[40, 30, 20, 10]
5
miss
done
