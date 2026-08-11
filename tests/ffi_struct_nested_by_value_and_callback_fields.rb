# A layout field can be another struct BY VALUE, or a declared callback.
# Reading a nested field yields the inner class VIEWING the same bytes (a
# write through the view mutates the parent); assigning one copies bytes,
# both oracle-verified against the gem. A callback field reads back as an
# FFI::Function over the stored code pointer, and accepts a callable on
# write -- the closure is kept alive by the struct.
require "ffi"

class Inner < FFI::Struct
  layout :a, :uint8, :b, :uint32
end
class Outer < FFI::Struct
  layout :x, :uint8, :inner, Inner, :y, :uint8
end

puts Outer.offset_of(:inner)
puts Outer.offset_of(:y)
puts Outer.size
o = Outer.new
o[:inner][:b] = 77
puts o[:inner][:b]
view = o[:inner]
view[:a] = 5
puts o[:inner][:a]
puts o[:inner].class
puts o[:inner].to_ptr.address - o.to_ptr.address

i2 = Inner.new
i2[:a] = 3
i2[:b] = 9
o[:inner] = i2
puts o[:inner][:b]
i2[:b] = 1
puts o[:inner][:b]

module M
  extend FFI::Library
  callback :adder_t, [:int, :int], :int
  class S < FFI::Struct
    layout :fn, :adder_t, :tail, :uint8
  end
end

s = M::S.new
puts M::S.offset_of(:tail)
s[:fn] = proc { |x, y| x + y }
puts s[:fn].class
puts s[:fn].call(20, 22)
