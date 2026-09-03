# Ported from spinel's :float_array/:int_array bulk-transfer specs (#474) to
# the real ffi gem API: a Ruby array's numeric contents cross the FFI boundary
# as a contiguous C buffer. write_array_of_* fills an FFI::MemoryPointer,
# memcpy moves the raw bytes C-side, and read_array_of_* recovers the
# elements -- the same bulk-transfer contract the original declared.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :memcpy, [:pointer, :pointer, :size_t], :pointer
end

src = FFI::MemoryPointer.new(:double, 3)
src.write_array_of_double([1.5, 2.5, 3.5])
dst = FFI::MemoryPointer.new(:double, 3)
LibC.memcpy(dst, src, 24)
puts dst.read_array_of_double(3).inspect

isrc = FFI::MemoryPointer.new(:int64, 2)
isrc.write_array_of_int64([65, 66])
idst = FFI::MemoryPointer.new(:int64, 2)
LibC.memcpy(idst, isrc, 16)
puts idst.read_array_of_int64(2).inspect

puts "ok"
__END__
[1.5, 2.5, 3.5]
[65, 66]
ok
