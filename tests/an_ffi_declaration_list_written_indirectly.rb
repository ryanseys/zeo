# Three spellings that put the same declaration list through an expression
# before the directive sees it. All are compile-time values, so all lower to
# the layout the program computes.
#
#   layout(*[ ... ])              the flat pair list, splatted (fzeet,
#                                 windows_gui and ffi-wingui-core all spell
#                                 NONCLIENTMETRICS this way)
#   layout(*{ ... }.to_a.flatten) the documented hash form, flattened into
#                                 pairs and splatted back apart (voicevox)
#   [:pointer, name = :string]    an assignment mid-argument-list, whose VALUE
#                                 is the type -- ffi-tk keeps the local for a
#                                 later declaration
require "ffi"

class Metrics < FFI::Struct
  layout(*[
    :cbSize, :uint,
    :iBorderWidth, :int,
    :iScrollWidth, :int
  ])
end

class Options < FFI::Struct
  layout(*{
    mode: :int32,
    threads: :int16,
    load_all: :bool
  }.to_a.flatten)
end

module Libc
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :zeo_memcpy, :memcpy, [:pointer, :pointer, size = :size_t], :pointer
end

p [Metrics.size, Metrics.members, Metrics.offset_of(:iScrollWidth)]
p [Options.size, Options.members, Options.offset_of(:load_all)]

m = Metrics.new
m[:iBorderWidth] = 3
p m[:iBorderWidth]

o = Options.new
o[:load_all] = true
p [o[:load_all], o[:threads]]

src = FFI::MemoryPointer.new(4)
src.put_bytes(0, "abc\0")
dst = FFI::MemoryPointer.new(4)
Libc.zeo_memcpy(dst, src, 4)
p dst.read_string

# The assignment's local belongs to the module body it was written in, so it
# is not visible out here -- the same NameError ruby raises.
begin
  p size
rescue NameError => e
  puts "size: #{e.class}"
end
