# An element inside a container compares by CRuby's `rb_equal`: the SAME
# object is equal to itself, and `==` is never asked. The `==` OPERATOR
# takes no such step, so the two answers differ exactly where identity and
# `==` disagree -- which is what makes `[n] == [n]` true while `n == n` is
# false for a NaN.
#
# Before the split, every container comparison went through the operator,
# so a NaN anywhere inside one silently made it unequal to itself -- and a
# NaN inside a container is what a YAML or JSON round trip produces from
# `.nan`.
#
# ONE ROW IS DELIBERATELY ABSENT. CRuby heap-allocates a NaN (it is outside
# the flonum range), so two SEPARATELY COMPUTED NaNs are different objects
# and `[0.0/0.0] == [0.0/0.0]` is false there. Zeo's Float is an immediate,
# so the two are one value and the answer is true. Giving a Float allocation
# identity to recover that costs the whole numeric representation, and every
# row below -- the ones a program actually writes -- already agrees.
n = Float::NAN

# The operator keeps IEEE's answer.
p n == n
p n.equal?(n)

# Every container reads the element rule instead.
p [n] == [n]
p [Float::NAN] == [Float::NAN]
p({ a: n } == { a: n })
p [[n]] == [[n]]
p({ n => 1 } == { n => 1 })
p [n, [n, { a: n }]] == [n, [n, { a: n }]]

# ... and so does every search that CRuby drives with `rb_equal`.
p [n].include?(n)
p [n].index(n)
p [n].rindex(n)
p [n].count(n)
p [n].delete(n)
p [[n, 1]].assoc(n)
p [[1, n]].rassoc(n)
p({ a: n }.value?(n))
p({ a: n }.key(n))
p({ a: n }.assoc(:a))
p({ a: n }.rassoc(n))
p({ a: n } <= { a: n })
p [n].each_entry.include?(n)
p [n].each_entry.count(n)
p [n].each_entry.find_index(n)

S = Struct.new(:x)
s = S.new(n)
p s == s
p S.new(n) == S.new(n)

# A user `==` that answers false for its own receiver loses to identity the
# same way -- the container never asks.
class Never
  def ==(_other) = false
  def inspect = "Never"
end
q = Never.new
p q == q
p [q] == [q]
p [q].include?(q)
p [q].index(q)
__END__
false
true
true
true
true
true
true
true
true
0
0
1
NaN
[NaN, 1]
[1, NaN]
true
:a
[:a, NaN]
[:a, NaN]
true
true
1
0
true
true
false
true
true
0
