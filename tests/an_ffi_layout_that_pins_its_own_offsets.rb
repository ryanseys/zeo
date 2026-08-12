# ruby-ffi's third element per field: an explicit byte OFFSET. Every Win32
# header struct in winwindow is written that way, and readline-ffi, PAPI,
# musicbeeipc, snarl and windows_gui all spell at least one. zeo read the
# argument list as `:name, :type` pairs only, so an odd count was an outright
# rejection.
#
# The number the declaration gives wins, however it sits against the field's
# own alignment. Size and alignment still come from the fields: this struct is
# 8 bytes aligned to 4, not 8 aligned to 2 -- oracle-verified, along with
# `alignment` itself, which the synthesized class did not answer at all.
require "ffi"

class Header < FFI::Struct
  layout :kind, :int16, 0,
         :size, :int32, 2,
         :tail, :int16, 6
end

p Header.size
p Header.alignment
p Header.offset_of(:size)
p Header.members

h = Header.new
h[:kind] = 3
h[:size] = 70_000
h[:tail] = -1
p [h[:kind], h[:size], h[:tail]]

# A packed layout, with no pinned offsets, still lays out the way it always
# did: each field rounded up to its own alignment.
class Packed < FFI::Struct
  layout :a, :int8, :b, :int32, :c, :int8
end

p Packed.size
p Packed.offset_of(:b)
p Packed.offset_of(:c)

# Unrelated to offsets, and in the same neighbourhood of "the declaration is
# not all literal": `**opts` where the class body set `opts = { blocking: true }`
# above. cztop, mosq, jansson and czmq-ffi-gen each hoist the one option they
# share into a local and splat it into every declaration.
module Libc
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  opts = { blocking: true }
  attach_function :labs, [:long], :long, **opts
  attach_function :abs, [:int], :int
end

p Libc.labs(-9)
p Libc.abs(-3)
