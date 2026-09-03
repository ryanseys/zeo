# An int array whose static type poly-collapsed (a param union widened it)
# must still marshal its element data at an FFI array boundary, and a poly
# value holding a non-array must raise instead of marshalling NULL. Ported
# from spinel's :int_array spec to the real ffi gem API: the array's elements
# land in an FFI::MemoryPointer via write_array_of_int64, and the observer is
# read_string over the marshalled bytes -- int64 65 is "A\0..." little-endian,
# so a correct marshal reads back "A".
require "ffi"

def widen(a)
  a
end

widen(["x", "y"])              # the trigger: widen's param/return go poly

buf = FFI::MemoryPointer.new(:int64, 2)
buf.write_array_of_int64(widen([65, 66]))
p buf.read_string

# a poly value holding a non-array raises loudly instead of marshalling NULL
begin
  buf.write_array_of_int64(widen("not an array"))
  puts "no raise"
rescue TypeError => e
  puts e.class
end
__END__
"A"
TypeError
