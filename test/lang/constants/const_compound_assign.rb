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
__END__
2
7
20
3
2
16
0
5
99
99
#@ stderr
lang/constants/const_compound_assign.rb:6: warning: already initialized constant A
lang/constants/const_compound_assign.rb:5: warning: previous definition of A was here
lang/constants/const_compound_assign.rb:9: warning: already initialized constant B
lang/constants/const_compound_assign.rb:8: warning: previous definition of B was here
lang/constants/const_compound_assign.rb:12: warning: already initialized constant C
lang/constants/const_compound_assign.rb:11: warning: previous definition of C was here
lang/constants/const_compound_assign.rb:15: warning: already initialized constant D
lang/constants/const_compound_assign.rb:14: warning: previous definition of D was here
lang/constants/const_compound_assign.rb:18: warning: already initialized constant E
lang/constants/const_compound_assign.rb:17: warning: previous definition of E was here
lang/constants/const_compound_assign.rb:21: warning: already initialized constant F
lang/constants/const_compound_assign.rb:20: warning: previous definition of F was here
lang/constants/const_compound_assign.rb:30: warning: already initialized constant G
lang/constants/const_compound_assign.rb:29: warning: previous definition of G was here
lang/constants/const_compound_assign.rb:33: warning: already initialized constant H
lang/constants/const_compound_assign.rb:32: warning: previous definition of H was here
