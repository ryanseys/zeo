# FFI `FFI::MemoryPointer`: an owned heap buffer with typed
# read/write accessors, pointer arithmetic, typed arrays, `from_string`, and
# an out-of-bounds `IndexError` -- byte-identical to `ffi 1.17.4`.

require "ffi"
p = FFI::MemoryPointer.new(:int, 3)
p.put_int(0, 10); p.put_int(4, 20); p.put_int(8, 30)
puts p.get_int(0)
puts p.get_int(4)
puts (p + 8).read_int
puts p.size
p.write_array_of_int([7, 8, 9])
puts p.read_array_of_int(3).inspect
sp = FFI::MemoryPointer.from_string("hi there")
puts sp.read_string
puts sp.size
d = FFI::MemoryPointer.new(:double, 1)
d.write_double(3.5)
puts d.read_double
begin
  FFI::MemoryPointer.new(:int, 1).get_int(4)
  puts "no error"
rescue IndexError
  puts "IndexError"
end
__END__
10
20
30
12
[7, 8, 9]
hi there
9
3.5
IndexError
