# A multiple assignment inside a method body that gets yield-inlined into its
# caller: every local-target emission (fixed targets, the rest, poly gets)
# must go through the inline rename table -- the declarations are renamed,
# so an unrenamed assignment references an undeclared C variable.
def pair
  a, b = yield
  [b, a]
end
p(pair { [1, 2] })
def rest_only
  *r = *yield
  r
end
p(rest_only { [3, 4] })
def spread
  x, y, *z = *yield
  [x, y, z]
end
p(spread { [1, 2, 3, 4] })
