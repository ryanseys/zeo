# `a, = rhs` is prism's ImplicitRestNode -- an anonymous rest that takes
# the named lefts and discards the rest, exactly like `a, *_ = rhs`. This
# is the `spec_tuples, = fetcher.spec_for_dependency` shape rubygems uses.

a, = [1, 2, 3]
puts a                       # 1

first, = 5                   # non-array RHS: first element is the value
puts first                   # 5

def pair; [10, 20]; end
loc, = pair
puts loc                     # 10

x, y, = [1, 2, 3, 4]         # two named, discard the rest
puts [x, y].inspect          # [1, 2]

# Implicit rest composes with everything: nested, ivars, index targets,
# a splat RHS, and the assignment's own return value.
a2, (b2,) = 1, [2, 9]
puts [a2, b2].inspect        # [1, 2]

arr = [7, 8, 9]
z, = *arr
puts z                       # 7

h = {}
h[:k], = [11, 22]
puts h.inspect               # {k: 11}

v = (q, = [30, 40])          # a multi-assign evaluates to its RHS
puts v.inspect               # [30, 40]
puts q                       # 30
__END__
1
5
10
[1, 2]
[1, 2]
7
{k: 11}
[30, 40]
30
