# Ported from spinel's ffi_const to the real ffi gem API: the gem has no
# ffi_const directive -- C-side flag values are plain Ruby constants on the
# library module, combinable with the usual Integer bit ops.
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
