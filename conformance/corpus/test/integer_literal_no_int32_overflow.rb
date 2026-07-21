# Spinel's integers are mrb_int (long long); Ruby integers don't
# overflow in the same range. But emitting a plain `24329 * 256 *
# 500` in C lets the C compiler fold the constant using `int`
# arithmetic, overflowing at 3,114,112,000 to a wrong negative
# result on 32-bit-int platforms.
#
# Surfaced via optcarrot's `APU::Mixer::TND_1 = 100 * 24329 * 256
# / 500`-shaped constant expressions whose intermediate products
# clear int32 max. Same hazard for any constant-init expression
# of integer literals whose folded result exceeds INT_MAX.
#
# Fix: emit the `LL` suffix on every IntegerNode literal so
# operands carry `long long` type at the C level. Runtime
# expressions assigned to `mrb_int` slots already promoted
# implicitly; only constant-init contexts (where the C compiler
# folds before any assignment) saw the overflow.

# Each chain straddles int32: max 2^31 - 1 = 2,147,483,647.
A = 24329 * 256 * 500           # 3,114,112,000
B = 100000 * 100000              # 10,000,000,000
C = 65535 * 65535                # 4,294,836,225
D = 2 ** 40                      # 1,099,511,627,776
E = 1_000_000_000 + 1_000_000_000 # 2,000,000,000 — sum overflow

puts A
puts B
puts C
puts D
puts E
