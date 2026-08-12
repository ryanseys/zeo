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

# A third literal-strictness shape in the same family: an inline array's
# element COUNT held in a constant the class body computed. librtmp writes
# `RTMP_BUFFER_CACHE_SIZE = (16*1024)` four lines above the field it sizes, and
# the count is what decides where every following field starts -- so it has to
# fold at LOWERING time. Only a bare integer literal used to be recorded.
module Net
  BUF = (16 * 1024)
  TAG = (1 << 3)

  class SockBuf < FFI::Struct
    layout :sock, :int,
           :buf, [:char, BUF],
           :tag, [:uint8, TAG]
  end
end

p Net::SockBuf.size
p Net::SockBuf.offset_of(:tag)

# And the argument LIST itself, when the gem did not want to spell a prototype
# twice: `[:pointer] * 13` (rbmetis' METIS bindings, csspool's croco
# callbacks) and a body-local `%i[...]` (ires).
module Repeated
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  params = %i[int]
  attach_function :abs, params, :int
  attach_function :labs, [:long] * 1, :long
  callback :pair, [:pointer] * 2, :void
end

p Repeated.abs(-4)
p Repeated.labs(-5)
