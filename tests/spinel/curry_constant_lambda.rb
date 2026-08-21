# `curry` walks the chain back to the base proc to learn its arity, which is
# how it knows which application completes it. The walk resolved a lambda held
# in a LOCAL and not one held in a CONSTANT, so a curried constant never
# realized: every application answered another Proc (#4017).
F = ->(a, b) { a + b }
p F.curry[1][2]
p F.curry.(1).(2)
p F.curry[1].call(2)
p F.curry[1, 2]

# a local copied from the constant resolves through it
G = F
p G.curry[1][2]
g = F
p g.curry[1][2]

# the argument type is irrelevant, and so is the arity
R = ->(r, v) { r.cover?(v) }
p R.curry[(1..9)][5]
S = ->(a, b, c) { a + b + c }
p S.curry[1][2][3]
p S.curry[1, 2][3]
T = ->(a) { a * 2 }
p T.curry[21]

# a partial application is still a Proc
p F.curry[1].class
p F.curry.class

# and the local-held lambda keeps working
f = ->(a, b) { a + b }
p f.curry[1][2]
