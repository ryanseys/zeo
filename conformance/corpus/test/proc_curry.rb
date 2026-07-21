# Proc#curry completion typing: a curried application stays a curry until it
# reaches the proc's arity, when it realizes to the proc's (int) result -- so a
# completed curry is a first-class int (printable with `p`, usable in
# arithmetic), while a partial application is still a curry awaiting more args.
add = ->(a, b) { a + b }
sub = ->(a, b) { a - b }
mul3 = ->(a, b, c) { a * b * c }

# direct chains realize to int
p add.curry[5][3]                 # 8
p sub.curry[10][3]                # 7
p mul3.curry[2][3][4]             # 24

# a completed curry is an ordinary int value
total = add.curry[10][20]
p total + 1                       # 31
p add.curry[2][3] * 10            # 50
p [add.curry[1][2], add.curry[3][4]]  # [3, 7]

# partial applications held in variables, completed later
add5 = add.curry[5]
p add5[3]                         # 8
p add5[100]                       # 105

f = mul3.curry[2]
g = f[3]
p g[4]                            # 24
p f[5][6]                         # 60

# empty curry then full application
cc = add.curry
p cc[10][20]                      # 30

# .call / .() forms also complete
p add.curry.call(7).call(8)       # 15
