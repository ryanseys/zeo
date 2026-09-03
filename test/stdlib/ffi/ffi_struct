# FFI::Struct through the real ffi gem API (ported from spinel's ffi_struct
# DSL): `layout` declares the C fields, `[]`/`[]=` read and write them, and a
# C function can fill a struct through its pointer.
require "ffi"

class Point < FFI::Struct
  layout :x, :long, :y, :long
end
class Mix < FFI::Struct
  layout :a, :int, :d, :double
end
class Node < FFI::Struct
  layout :val, :int, :next, :pointer
end
class Timeval < FFI::Struct
  layout :tv_sec, :long, :tv_usec, :long
end

module M
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :gettimeofday, [:pointer, :pointer], :int
end

# scalar fields round-trip
pt = Point.new
pt[:x] = 3
pt[:y] = 7
puts pt[:x]
puts pt[:y]
puts(pt[:x] + pt[:y])

# mixed int + double
m = Mix.new
m[:a] = 42
m[:d] = 3.5
puts m[:a]
puts m[:d]

# pointer fields chain, NULL reads back as nil-equal
a = Node.new
b = Node.new
a[:val] = 11
a[:next] = b.pointer
puts a[:val]
puts(a[:next] != nil)
puts(b[:next] == nil)

# interop: gettimeofday fills the struct through its pointer
tv = Timeval.new
M.gettimeofday(tv.pointer, nil)
puts(tv[:tv_sec] > 0)

# a pointer field can carry a C string; read it back through read_string
label = FFI::MemoryPointer.from_string("hello")
nm = Node.new
nm[:next] = label
nm[:val] = 5
puts nm[:next].read_string
puts nm[:val]
__END__
3
7
10
42
3.5
11
true
true
true
hello
5
