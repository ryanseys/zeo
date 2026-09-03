# The declaration spellings the corpus writes that the strict forms missed:
# a String function name (the gem calls `.to_sym` on it), and
# `ffi_lib FFI::CURRENT_PROCESS` (symbols from the already-linked image --
# what a `lib` of `None` emits).

require "ffi"
module L
  extend FFI::Library
  ffi_lib FFI::CURRENT_PROCESS
  attach_function "abs", [:int], :int
end
puts L.abs(-9)
__END__
9
