# Compound assignment to a constant (`A += 2`, `A ||= 2`, `A &&= 2`).
# The constant slot is mutated at runtime, so its reads must load the
# live value rather than fold the declaration literal. ||=/&&= follow
# Ruby truthiness: 0 is truthy, so `A ||= x` keeps a zero constant.
A = 0
A += 2
p A          # 2
B = 10
B -= 3
p B          # 7
C = 4
C *= 5
p C          # 20
D = 17
D /= 5
p D          # 3
E = 17
E %= 5
p E          # 2
F = 1
F <<= 4
p F          # 16
ZERO = 0
ZERO ||= 99
p ZERO       # 0 (0 is truthy in Ruby)
FIVE = 5
FIVE ||= 99
p FIVE       # 5
G = 0
G &&= 99
p G          # 99 (0 is truthy, so &&= reassigns)
H = 7
H &&= 99
p H          # 99
