# C-side flag values are ordinary Ruby constants on the library module, and
# they combine with the usual Integer bit operations.
require "ffi"

module Flags
  extend FFI::Library
  READ  = 1
  WRITE = 2
  EXEC  = 4
  MASK  = 0xff
end

puts Flags::READ
puts Flags::WRITE
puts Flags::EXEC
puts Flags::MASK
puts(Flags::READ | Flags::WRITE | Flags::EXEC)
__END__
1
2
4
255
7
