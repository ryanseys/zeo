# The shape under test is the strlen
# binding those flags decorated.
require "ffi"

module Pathy
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strlen, [:string], :size_t
end

puts Pathy.strlen("hello")
puts Pathy.strlen("")
__END__
5
0
