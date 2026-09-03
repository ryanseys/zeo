# An inline array field whose element COUNT is `OtherStruct.size * N`.
#
# zeo laid `OtherStruct` out itself, so its extent is as much a compile-time
# constant as the literal the gem could have written -- j-law-ruby sizes every
# one of its inline storage arrays this way, and zeo refused the whole file
# because the count "must be an integer literal".
require "ffi"

module Lib
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  MAX_TIERS = 3

  class Step < FFI::Struct
    layout :label, [:char, 8],
           :amount, :uint64
  end

  class Fee < FFI::Struct
    layout :total, :uint64,
           :storage, [:char, Step.size * MAX_TIERS],
           :len, :int
  end

  # `.alignment` folds for the same reason.
  class Padded < FFI::Struct
    layout :head, :uint8,
           :pad, [:char, Step.alignment],
           :tail, :uint64
  end
end

p Lib::Step.size
p Lib::Step.alignment
p Lib::Fee.size
p Lib::Fee.offset_of(:storage)
p Lib::Fee.offset_of(:len)
p Lib::Padded.offset_of(:tail)

f = Lib::Fee.new
f[:total] = 7
f[:len] = 2
p f[:total]
p f[:len]
__END__
16
8
64
8
56
16
7
2
