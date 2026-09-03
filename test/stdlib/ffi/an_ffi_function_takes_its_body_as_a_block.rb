# The ffi gem takes an `FFI::Function`'s body as a BLOCK as readily as a Proc
# in the third position -- the two spellings build the same closure. zeo's
# constructor counted the block as a missing argument.

require "ffi"
module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  callback :cmp, [:pointer, :pointer], :int
  attach_function :qsort, [:pointer, :size_t, :size_t, :cmp], :void
end
fn = FFI::Function.new(:int, [:pointer, :pointer]) { |a, b| a.read_int <=> b.read_int }
buf = FFI::MemoryPointer.new(:int, 4)
buf.write_array_of_int([3, 1, 2, 0])
L.qsort(buf, 4, 4, fn)
p buf.read_array_of_int(4)

desc = FFI::Function.new(:int, [:pointer, :pointer], proc { |a, b| b.read_int <=> a.read_int })
L.qsort(buf, 4, 4, desc)
p buf.read_array_of_int(4)

# A callback may answer nothing, and say so.
sink = FFI::Function.new(:void, [:pointer, :pointer]) { |_a, _b| nil }
p sink.class
__END__
[0, 1, 2, 3]
[3, 2, 1, 0]
FFI::Function
