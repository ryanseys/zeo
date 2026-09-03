# #688 (ported to the real ffi gem API): empty `[]` followed by pushes of
# FFI :pointer values must behave as an array of pointers, not silently
# collapse to an int-array representation that would round-trip the pointer
# through an integer.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :malloc, [:size_t], :pointer
  attach_function :free,   [:pointer], :void
end

p1 = LibC.malloc(64)
p2 = LibC.malloc(64)

arr = []
arr.push(p1)
arr.push(p2)

puts arr.length

# Read back the pointers and confirm they're identical to what we pushed.
got1 = arr[0]
got2 = arr[1]
puts (got1 == p1) ? "p1=match" : "p1=MISMATCH"
puts (got2 == p2) ? "p2=match" : "p2=MISMATCH"

LibC.free(p1)
LibC.free(p2)
__END__
2
p1=match
p2=match
