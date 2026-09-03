# `FFI.typedef :existing, :alias` writes the gem's GLOBAL type registry --
# visible to every library module and struct layout that runs after it, from
# whatever plain namespace it was written in (puppet declares the whole Win32
# vocabulary this way in one file and spends it across the rest).
require "ffi"

module Vocabulary
  FFI.typedef :uint16, :word
  FFI.typedef :uintptr_t, :handle
end

class S < FFI::Struct
  layout :w, :word, :h, :handle
end

module Lib
  extend FFI::Library
  ffi_lib "m"
  attach_function :my_labs, :labs, [:handle], :handle
  attach_function :my_fabs, :fabs, [:double], :double
end

puts S.offset_of(:h)
puts S.size
puts Lib.my_labs(7)
puts Lib.my_fabs(-1.5)
__END__
8
16
7
1.5
