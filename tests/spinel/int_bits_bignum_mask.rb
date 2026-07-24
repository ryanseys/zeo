# Integer#allbits?/#anybits?/#nobits? with a Bignum mask: an int receiver
# cannot cover a mask that exceeds int64 (allbits? is false); anybits?/nobits?
# test the mask's low 64 bits.
a = 14; b = 2 ** 100
p(a.allbits?(b))
p(a.anybits?(b))
p(a.nobits?(b))
p(a.allbits?(6))
p(a.anybits?(1))
p(a.nobits?(1))
p(255.anybits?(2 ** 64))
p(255.anybits?(2 ** 64 + 1))
