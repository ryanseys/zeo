# The masked-xorshift idiom under --int-overflow=promote: a value past 2^63
# stays a positive bignum, so `.to_f / 2**64` lands in [0,1) and `>>` shifts a
# non-negative value -- the stream matches CRuby draw for draw. (#3371)
#
# The state array is seeded with a bignum so it is not an INT array: a promoted
# value stored into an int-typed array is still truncated back to int64, which
# is a documented promote-mode gap (see docs/limitations.md).
def xorshift_uniform!(state)
  x = state[0]
  x = x ^ (x << 13)
  x = x & 0xFFFFFFFFFFFFFFFF
  x = x ^ (x >> 7)
  x = x ^ (x << 17)
  x = x & 0xFFFFFFFFFFFFFFFF
  state[0] = x
  (x.to_f / 18446744073709551616.0) + 1.0e-300
end
st = [104729, 0xFFFFFFFFFFFFFFFF]
6.times { puts xorshift_uniform!(st) }
p st[0]

# the pieces, on their own: a bignum keeps its width through ^ | & and >>
x = 17376399848315274067
p x >> 7
p x ^ (x >> 7)
p x | 1
p x & 0xFFFFFFFF
p (921264593961460563 << 17)
