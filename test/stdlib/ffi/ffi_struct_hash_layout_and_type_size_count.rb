# `layout` written as one hash (the gem's documented alternative), a
# root-anchored `::FFI::Struct` superclass, and an inline-array count
# computed from `FFI::Type::X.size` -- three corpus spellings in one layout.
#
# 4 + 4 + 2 bytes, rounded up to the 4-byte alignment.

require "ffi"
class Pt < ::FFI::Struct
  layout x: :int32, y: :int32, pad: [:uint8, 16 / ::FFI::Type::LONG.size]
end
p = Pt.new
p[:x] = 7
p[:y] = 35
puts p[:x] + p[:y]
puts Pt.size
__END__
42
12
