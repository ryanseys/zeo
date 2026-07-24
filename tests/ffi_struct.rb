# FFI::Struct + layout (#204 follow-on): C field offsets/alignment, [] / []=,
# size, offset_of, members -- synthesized over an FFI::MemoryPointer.
require "ffi"

class Point < FFI::Struct
  layout :x, :int, :y, :int, :label, :pointer
end

puts Point.size
puts Point.offset_of(:y)
puts Point.offset_of(:label)
puts Point.members.inspect

p = Point.new
p[:x] = 3
p[:y] = 7
puts p[:x]
puts p[:y]

class Mixed < FFI::Struct
  layout :flag, :int8, :count, :long, :ratio, :double
end
puts Mixed.size
puts Mixed.offset_of(:count)
puts Mixed.offset_of(:ratio)
