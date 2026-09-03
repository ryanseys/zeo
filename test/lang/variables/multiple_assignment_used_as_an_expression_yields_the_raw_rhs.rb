# `(a, b = rhs)` in value position yields the RHS verbatim, as CRuby does:
# a call returning an array yields that array (not a fresh copy), a bare
# scalar yields the scalar (not `[scalar]`), an implicit list yields the
# list. The destructuring still happens; the expression's own value is the
# untouched RHS. Regression: the sub-expression form used to yield `nil`,
# so an outer subscript/comparison hit `[]`/`<` on nil.

def two_ints; [10, 20]; end
result = (x, y = two_ints)
p x
p y
p result
first = (p1, p2, p3 = [7, 8, 9])[0]
p first
flag = (a, b = [3, 4])[0] == 3
p flag
p((c, d = 5))
p((e, f = 1, 2))
__END__
10
20
[10, 20]
7
true
5
[1, 2]
