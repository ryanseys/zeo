# #492 (ported to the real ffi gem API). An array literal whose elements are
# FFI :pointer returns (e.g. `[LibC.malloc(...)]`) must infer as an array of
# pointers, not collapse to an int-array representation -- push sites and
# element reads round-trip the pointer values intact.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :malloc, [:size_t], :pointer
  attach_function :free,   [:pointer], :void
end

class Bag
  attr_accessor :slots
  def initialize
    @slots = [LibC.malloc(8)]
  end
end

b = Bag.new
b.slots.push(LibC.malloc(8))
puts b.slots.length
b.slots.each { |p| LibC.free(p) }
__END__
2
