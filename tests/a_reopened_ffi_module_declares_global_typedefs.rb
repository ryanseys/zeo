# `module ::FFI; typedef :ulong, :XID; end` -- self IS the FFI module inside
# that body, so a bare `typedef` there is the same GLOBAL declaration
# `FFI.typedef` makes, visible to every library and struct that lowers after
# it. The x11 family fills the whole X vocabulary this way in one file and
# spends it across every other one.
require "ffi"

module ::FFI
  typedef :ulong, :XID
  # A typedef whose SOURCE is another global typedef resolves through it.
  typedef :XID, :Font
  typedef :int, :Bool
end

module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strlen, [:string], :XID
  attach_function :abs, [:Bool], :Bool
end

p L.strlen("hello")
p L.abs(-7)

class Glyph < FFI::Struct
  layout :id, :Font, :ok, :Bool
end

g = Glyph.new
g[:id] = 4_294_967_296
g[:ok] = 1
p Glyph.size
p g[:id]
p g[:ok]
puts "still running"
