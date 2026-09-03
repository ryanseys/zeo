# An INLINE array field (`layout :bytes, [:uint8, 4]`) stores its elements in
# place, so it is the field that decides where every following one starts --
# and reading it yields ruby's own proxy OVER that memory, not a copy.
#
# zeo refused the whole struct: `[:uint8, 4]` is not a symbol, so the layout
# never parsed. The offsets, the two proxy classes, and which of them answers
# `to_s` are all observable, so all three are matched here rather than
# approximated.
require "ffi"

class Mixed < FFI::Struct
  layout :a, [:int32, 3], :b, [:char, 4], :c, [:uint8, 2], :d, [:double, 2]
end

# C alignment: each array aligns to its ELEMENT, and the struct rounds up to
# its widest member.
p Mixed.size
p [Mixed.offset_of(:a), Mixed.offset_of(:b), Mixed.offset_of(:c), Mixed.offset_of(:d)]
p Mixed.members

# `CharArray` for an 8-bit element, `InlineArray` otherwise -- the gem's own
# split, and the reason only one of them answers `to_s`.
s = Mixed.new
p [s[:a].class, s[:b].class, s[:c].class, s[:d].class]
p [s[:a].size, s[:d].size]
p s[:b].respond_to?(:to_str)
p s[:a].respond_to?(:to_str)

# The proxy writes THROUGH to the struct's memory.
s[:a][1] = 9
p s[:a].to_a
s[:b][0] = 72
s[:b][1] = 105
p s[:b].to_s

# ...and a whole-array assignment is refused, as ruby refuses it.
begin
  s[:a] = [1, 2, 3]
rescue NotImplementedError => e
  p e.message
end

# An array field does not disturb the ordinary fields around it.
class Framed < FFI::Struct
  layout :tag, :int32, :bytes, [:uint8, 4], :tail, :int32
end
f = Framed.new
f[:tag] = 7
f[:tail] = 9
f[:bytes][3] = 255
p [f[:tag], f[:tail], f[:bytes].to_a]
p Framed.size

# The element count may be a constant this body already set -- C bindings spell
# array widths that way far more often than as bare numbers (sys-filesystem).
class Named < FFI::Struct
  NAME_LEN = 8
  layout :name, [:char, NAME_LEN], :id, :int32
end
p [Named.size, Named.offset_of(:id), Named.new[:name].size]

# `each`/`to_a` come from Enumerable over the same proxy.
n = Named.new
n[:name][0] = 90
p n[:name].each_with_index.first(2)
p n[:name].to_a.length

# A SECOND struct with an array field reuses the same proxy classes rather than
# redefining them.
class Other < FFI::Struct
  layout :v, [:uint8, 2]
end
p Other.new[:v].class == Framed.new[:bytes].class
__END__
40
[0, 12, 16, 24]
[:a, :b, :c, :d]
[FFI::Struct::InlineArray, FFI::StructLayout::CharArray, FFI::StructLayout::CharArray, FFI::Struct::InlineArray]
[3, 2]
true
false
[0, 9, 0]
"Hi"
"cannot set array field"
[7, 9, [0, 0, 0, 255]]
12
[12, 8, 8]
[[90, 0], [0, 1]]
8
true
