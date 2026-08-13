# `FFI::Pointer`'s typed ARRAY accessors, in all four spellings ruby-ffi has:
# `read_`/`write_` start at offset 0, `get_`/`put_` take one. zeo had six of
# the ninety-odd, so a gem reading a C array back (`put_array_of_int32`,
# `get_array_of_pointer`, ...) hit a NoMethodError at run time.
#
# Also here: `#clear`, `#type_size`, `#order`, `#read_string_length` and the
# generic `read_array_of_type`/`write_array_of_type` pair.
require "ffi"

ptr = FFI::MemoryPointer.new(:int32, 4)
ptr.write_array_of_int32([1, 2, 3, 4])
p ptr.read_array_of_int32(4)
p ptr.get_array_of_int32(4, 2)
ptr.put_array_of_int32(8, [30, 40])
p ptr.read_array_of_int(4)
p ptr.type_size
p ptr.size

# Every width, signed and unsigned, through the offset spellings.
buf = FFI::MemoryPointer.new(:uint8, 64)
buf.put_array_of_int8(0, [-1, 2])
p buf.get_array_of_int8(0, 2)
p buf.get_array_of_uint8(0, 2)
buf.put_array_of_int16(8, [-300, 300])
p buf.get_array_of_int16(8, 2)
p buf.get_array_of_uint16(8, 2)
buf.put_array_of_int64(16, [-5, 5])
p buf.get_array_of_int64(16, 2)
buf.put_array_of_uint64(16, [7, 8])
p buf.get_array_of_uint64(16, 2)
buf.put_array_of_long(32, [11, 12])
p buf.get_array_of_long(32, 2)
buf.put_array_of_ushort(48, [65535])
p buf.get_array_of_ushort(48, 1)

# Floats, in both widths and both alias spellings.
f = FFI::MemoryPointer.new(:double, 4)
f.write_array_of_double([1.5, 2.5])
p f.read_array_of_double(2)
f.put_array_of_float64(16, [3.5])
p f.get_array_of_double(16, 1)
g = FFI::MemoryPointer.new(:float, 4)
g.write_array_of_float([0.5, 1.25])
p g.read_array_of_float(2)
p g.get_array_of_float32(0, 2)

# Pointers, by value and back.
slots = FFI::MemoryPointer.new(:pointer, 2)
a = FFI::MemoryPointer.new(:int32, 1)
a.write_int32(42)
slots.write_array_of_pointer([a, FFI::Pointer.new(0)])
back = slots.read_array_of_pointer(2)
p back.length
p back[0].read_int32
p back[1].null?
p slots.get_array_of_pointer(0, 1).first.read_int32

# The generic pair, over the same accessors. Not symmetric: ruby-ffi's writer
# is sent `(offset, value)` while its reader walks a stepped pointer, so the
# writer names a `put_` and the reader a `read_`.
t = FFI::MemoryPointer.new(:int32, 3)
t.write_array_of_type(:int32, :put_int32, [7, 8, 9])
p t.read_array_of_type(:int32, :read_int32, 3)

# `#clear` zeroes the whole extent.
t.clear
p t.read_array_of_int32(3)

# `#read_string_length` takes the bytes verbatim, NULs and all.
s = FFI::MemoryPointer.from_string("ab\0cd")
p s.read_string_length(5)
p s.read_string

# Byte order on a little-endian target.
p ptr.order
p ptr.order(:little).equal?(ptr)

# An array of C strings.
argv = FFI::MemoryPointer.new(:pointer, 2)
one = FFI::MemoryPointer.from_string("one")
two = FFI::MemoryPointer.from_string("two")
argv.write_array_of_pointer([one, two])
p argv.read_array_of_string(2)
p argv.get_array_of_string(0, 2)

# Out of bounds stays the gem's IndexError.
begin
  ptr.read_array_of_int32(1000)
rescue IndexError => e
  puts "IndexError"
end
