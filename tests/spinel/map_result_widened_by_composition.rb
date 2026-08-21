# A local assigned a `map` whose block value type widens LATE. Composing the
# same method with `>>` widens its parameters to poly -- the composition's
# intermediate value is only known at run time -- so its return widens too and
# the map builds a poly array. The local had already settled on the typed array
# kind and kept it, so the assignment did not compile (#4009):
#
#   error: assignment to 'sp_IntArray *' from incompatible pointer type
#          'sp_PolyArray *'
#
# `&method(:m)` is not the trigger: a literal block calling the same method
# fails identically, and either statement alone compiles.
def double(n) = n * 2

v = [1, 2, 3].map(&method(:double))
p v
p (method(:double).to_proc >> ->(x) { x + 1 }).call(3)

def triple(n) = n * 3
w = [1, 2, 3].map { |x| triple(x) }
p w
p (method(:triple).to_proc << ->(x) { x + 1 }).call(3)

# neither statement alone was ever a problem
def quad(n) = n * 4
p [1, 2, 3].map(&method(:quad))
p method(:quad).to_proc.call(3)

# and an ordinary typed map keeps its kind
z = [1, 2, 3].map { |x| x * 2 }
p z
p z.sum
