# DECIDED DIVERGENCE. `Marshal.dump(BigDecimal(...))` writes the same block
# shape and the same value as ruby, but a different precision word.
#
# The format is `"<precision>:<to_s>"`, and ruby's precision word is an
# ALLOCATION artifact rather than a property of the number. Measured against
# ruby 4.0.6:
#
#     BigDecimal("1")        -> "9:0.1e1"
#     BigDecimal("1.25")     -> "18:0.125e1"
#     BigDecimal("-0.5e10")  -> "9:-0.5e10"
#     BigDecimal("1234567890") -> "18:0.123456789e10"
#
# `1` and `1.25` are both one 9-digit limb of magnitude, and `-0.5e10` has
# FEWER significant digits than `1.25` yet a smaller word. The number tracks
# the length of the STRING that built the value, because `VpAlloc` sizes the
# limb array from it before parsing. `#precision` does not predict it either:
# `-0.5e10` reports 10 and dumps 9.
#
# zeo writes `#precision` rounded up to a multiple of 9, which is a fact about
# the number rather than about how it was spelled. It is larger than ruby's
# where the value is wide (`1e100` reports 101 digits of precision, so 108),
# and smaller where ruby over-allocated from a long literal.
#
# **Nothing reads the word.** Both loaders split on the first colon and parse
# the rest, so a dump crosses between the two runtimes in either direction --
# which the rows below check, rather than checking bytes zeo has decided not
# to reproduce. Reproducing them would mean porting `VpAlloc`'s sizing rule,
# and it would make the dump depend on the source spelling of a literal.
require "bigdecimal"

VALUES = ["1", "1.25", "-0.5e10", "0", "NaN", "Infinity", "1e100", "0.000001"]

VALUES.each do |s|
  b = BigDecimal(s)
  puts "#{s}\t#{b._dump.inspect}\t#{BigDecimal._load(b._dump).to_s}"
end

# The round trip through Marshal itself, which is what a program does.
VALUES.each do |s|
  b = BigDecimal(s)
  back = Marshal.load(Marshal.dump(b))
  # NaN is never equal to itself, so compare the printed form there.
  same = b.nan? ? back.nan? : back == b
  puts "#{s}\t#{back.to_s}\t#{same}"
end

# A dump ruby WROTE loads here, word and all -- the word is advisory on both
# sides, so the exact bytes ruby 4.0.6 produced for `BigDecimal("1.25")` are
# pinned here as a literal.
puts Marshal.load("\x04\bu:\x0FBigDecimal\x0F18:0.125e1").to_s

# The block shape is ruby's: a bare `u`, with no `I` wrapper. `_dump` answers
# an ASCII-8BIT string, so Marshal writes no encoding ivar -- its `Encoding`
# sibling deliberately does the opposite, and both match ruby.
puts Marshal.dump(BigDecimal("1")).inspect
