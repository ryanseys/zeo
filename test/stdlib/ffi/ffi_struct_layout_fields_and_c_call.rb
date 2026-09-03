# FFI `FFI::Struct` + `layout`: a `class T < FFI::Struct`
# with a `layout` gets synthesized `[]`/`[]=`/`size`/`offset_of`/`members`
# over an owned `FFI::MemoryPointer`, with C field offsets/alignment. The
# struct is auto-converted to its pointer when passed to a C `:pointer`
# argument (`gettimeofday` fills `tv_sec`). Byte-identical to `ffi 1.17.4`.

require "ffi"
class Timeval < FFI::Struct
  layout :tv_sec, :long, :tv_usec, :int
end
puts Timeval.size
puts Timeval.offset_of(:tv_usec)
puts Timeval.members.inspect
t = Timeval.new
t[:tv_sec] = 123
t[:tv_usec] = 456
puts t[:tv_sec]
puts t[:tv_usec]

class Mixed < FFI::Struct
  layout :a, :int8, :b, :long, :c, :int
end
puts Mixed.size
puts Mixed.offset_of(:b)
puts Mixed.offset_of(:c)

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :gettimeofday, [:pointer, :pointer], :int
end
tv = Timeval.new
C.gettimeofday(tv, nil)
puts(tv[:tv_sec] > 1_000_000)
__END__
16
8
[:tv_sec, :tv_usec]
123
456
24
8
16
true
