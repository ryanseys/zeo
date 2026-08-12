# FFI declaration spellings the lowering used to reject as "literal symbol/
# string required", together in one program:
#
#  - an INLINE `callback([...], ret)` in a type position -- the anonymous twin
#    of the named `callback :tag` declaration;
#  - the gem's DIRECTION annotations (`Struct.in`/`.out`, `Struct.ptr(:in)`) --
#    every one a plain pointer on the ABI, the direction only tunes the gem's
#    own marshaling copies;
#  - enum member values and inline-array counts read from QUALIFIED constants
#    (`Limits::FLAG_B`), not just body-local bare names;
#  - an inline array OF STRUCTS, each element a view over its slot in place.
require "ffi"

module Limits
  FLAG_B = 8
  PAIRS = 3
end

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :qsort,
                  [:pointer, :size_t, :size_t, callback([:pointer, :pointer], :int)],
                  :void
end

arr = FFI::MemoryPointer.new(:int32, 4)
arr.write_array_of_int32([40, 10, 30, 20])
C.qsort(arr, 4, 4, proc { |a, b| a.read_int32 <=> b.read_int32 })
p arr.read_array_of_int32(4)

# Direction annotations resolve to the same pointer type `.by_ref` does.
module Timing
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  class TimeVal < FFI::Struct
    layout :tv_sec, :int64,
           :tv_usec, :int64
  end

  attach_function :gettimeofday, [TimeVal.out, :pointer], :int
  attach_function :memcmp, [TimeVal.in, TimeVal.ptr(:in), :size_t], :int
end

tv = Timing::TimeVal.new
p Timing.gettimeofday(tv, nil)
p tv[:tv_sec] > 1_700_000_000
p Timing.memcmp(tv, tv, 16)

# An enum member's explicit value may be a QUALIFIED constant; the members
# after it keep auto-incrementing from it.
module E
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  enum :flags, [:a, 1, :b, Limits::FLAG_B, :c]
  attach_function :flag_abs, :abs, [:flags], :int
end
p E.flag_abs(:b)
p E.flag_abs(:c)

# An inline array of STRUCTS: elements sit in place, and reading one yields
# the struct class viewing its slot -- a write through the view writes the
# parent's memory.
class Pair < FFI::Struct
  layout :x, :int32, :y, :int32
end

class Board < FFI::Struct
  layout :count, :int32,
         :pairs, [Pair, Limits::PAIRS],
         :tail, :int8
end

b = Board.new
p Board.size
p [Board.offset_of(:pairs), Board.offset_of(:tail)]
b[:count] = 2
row = b[:pairs][1]
row[:x] = 7
row[:y] = 9
p [b[:pairs][1][:x], b[:pairs][1][:y]]
p [b[:pairs][0][:x], b[:pairs][2][:y]]
p b[:pairs].size
p b[:pairs].to_a.map { |pr| pr.class }
