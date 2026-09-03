# A `callback` whose parameter names an `FFI::Struct` subclass. zeo refused
# these outright, on the belief that ruby-ffi hands the Proc a Struct instance
# there and a raw pointer would be silently different behaviour.
#
# It hands a pointer. `StructByReference#from_native` would build a Struct, but
# the callback path never routes an argument through the Ruby data converter --
# the block below receives an `FFI::Pointer`, in ruby as in zeo. The old
# rejection even told the author to "take :pointer and wrap it yourself", which
# is exactly what the gem does for them.
#
# A callback RETURNING a struct class is still refused: nothing measured it,
# and a silently wrong conversion is worse than a refusal.
require "ffi"

module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  class Pair < FFI::Struct
    layout :a, :int, :b, :int
  end

  callback :cmp, [Pair, Pair], :int
  attach_function :qsort, [:pointer, :size_t, :size_t, :cmp], :void
end

seen = nil
compare = proc do |x, y|
  seen ||= x.class
  x.get_int32(0) <=> y.get_int32(0)
end

buf = FFI::MemoryPointer.new(:int, 4)
[30, 1, 20, 2].each_with_index { |v, i| buf.put_int32(i * 4, v) }
L.qsort(buf, 2, 8, compare)

p seen
p((0...4).map { |i| buf.get_int32(i * 4) })
__END__
FFI::Pointer
[20, 2, 30, 1]
