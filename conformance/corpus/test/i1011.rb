# i1011, ported to the real ffi gem API. The original exercised spinel's
# ffi_cflags compile-time string folding (__dir__, File.expand_path,
# String#+), which has no gem analogue; the surviving intent is the strlen
# binding those flags decorated.
require "ffi"

module Pathy
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strlen, [:string], :size_t
end

puts Pathy.strlen("hello")
puts Pathy.strlen("")
