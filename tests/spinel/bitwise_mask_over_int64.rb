# A bitwise op whose literal operand exceeds int64 (a 64-bit mask such as
# 0xFFFFFFFFFFFFFFFF, typed as a bigint) must still lower -- the result slot
# is int and takes the low-64 bit pattern. The xorshift64 / 64-bit-mask
# idiom. (#1513)
x = 12345
puts(x & 0xFFFFFFFFFFFFFFFF)        # 12345
puts(255 & 0xFFFFFFFFFFFFFFFF)      # 255
puts(0 & 0xFFFFFFFFFFFFFFFF)        # 0
y = 0xDEADBEEF
puts((y ^ 0x12345678) & 0xFFFFFFFFFFFFFFFF)   # 3432638615
# one xorshift64 step on a fixed seed (deterministic)
s = 88172645463325252
s ^= (s << 13) & 0xFFFFFFFFFFFFFFFF
s ^= (s >> 7)  & 0xFFFFFFFFFFFFFFFF
s ^= (s << 17) & 0xFFFFFFFFFFFFFFFF
puts(s & 0xFFFFFFFF)                # low-32 of the mixed state
