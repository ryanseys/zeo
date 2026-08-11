# An enum member's VALUE and an inline array's COUNT are compile-time
# integers to zeo -- they decide marshaling tables and field offsets. Both
# now fold through one folder that also reads constants the class body
# already set (`BASE`, `WIDTH * 2`). `FLAGS = enum :flags, [...]` is the
# gem's NAMED form spelled as an assignment: registered under the tag AND
# the constant, so both spellings work in later type positions.
require "ffi"

module Native
  extend FFI::Library
  BASE = 4
  FLAGS = enum :flags, [:read, (1 << 0), :write, BASE, :append]
  Mode = enum :ro, :rw, :extra

  class Rec < FFI::Struct
    WIDTH = 8
    layout :tag, FLAGS,
           :mode, Mode,
           :flg2, :flags,
           :name, [:uint8, WIDTH * 2]
  end
end

r = Native::Rec.new
r[:tag] = :write
r[:mode] = 2
r[:flg2] = :append
puts Native::Rec.offset_of(:mode)
puts Native::Rec.offset_of(:name)
puts Native::Rec.size
p r[:tag]
p r[:mode]
p r[:flg2]
