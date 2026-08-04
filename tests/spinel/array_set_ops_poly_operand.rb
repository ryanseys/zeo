# An Array set operation whose OPERAND is poly -- a value whose static type
# widened, not a poly array. The four diverged three ways: `&` and `|` had no
# arm and would not compile, `-` was typed as arithmetic and reached
# sp_poly_sub, which had no array case and answered "no implicit conversion of
# Array into Array" on two real Arrays, and only `+` was right. The operand is
# coerced at run time now: an Array becomes the poly array the set-op
# primitives take, anything else raises the TypeError CRuby raises. #3475.
def pick(flag) = flag ? [1, 2] : "x"
def picks(flag) = flag ? ["s", "t"] : 1
def bad(flag) = flag ? "x" : [1]

# poly-array receiver
a = [1, "s"]
b = pick(true)
p(a & b)
p(a | b)
p(a - b)
p(a + b)
p(a.intersection(b))
p(a.union(b))
p(a.difference(b))

# int-array receiver
i = [1, 2, 3]
p(i & b)
p(i | b)
p(i - b)

# str-array receiver
s = ["s", "t", "u"]
c = picks(true)
p(s & c)
p(s | c)
p(s - c)

# float-array receiver, poly holding an int array
f = [1.5, 2.5]
p((f - b).length)

# the operand is not an Array at run time: CRuby's TypeError
p((i & bad(true) rescue $!.class))
p((i | bad(true) rescue $!.class))
p((i - bad(true) rescue $!.class))

# and when it is
p(i & bad(false))

# empty operand
e = pick(false)
p([1, 2] - (e.is_a?(String) ? [] : []))
# The Rails shape the issue came from: a facade that delegates the set
# operations to its materialised records, with an untyped parameter.
class Relation
  def initialize(rows) = @rows = rows
  def to_a = @rows
  def +(other) = to_a + other
  def &(other) = to_a & other
  def |(other) = to_a | other
  def -(other) = to_a - other
end

r = Relation.new([1, "s", 2])
p(r + [2, 3])
p(r & [2, 3])
p(r | [2, 3])
p(r - [2, 3])
p((r & "nope" rescue $!.class))
