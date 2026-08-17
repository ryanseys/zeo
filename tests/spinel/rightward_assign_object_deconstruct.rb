# `obj => pattern` on a class that defines #deconstruct_keys / #deconstruct binds
# through the method, as case/in already did: the object used to fall to the
# untyped path, where every binding was skipped and stayed nil (#3955).
class N
  def deconstruct_keys(keys) = { kind: :binop, n: 1 }
end
N.new => { kind: }
p kind
N.new => { kind: k2, n: }
p [k2, n]

class A
  def deconstruct = [:x, "y", 3]
end
A.new => [a, b, cc]
p [a, b, cc]

class Ints
  def deconstruct = [1, 2, 3]
end
Ints.new => [h, *t]
p [h, t]

# Struct keeps binding through its synthesized members.
S = Struct.new(:kind, :n)
S.new(:binop, 1) => { kind: sk }
p sk
S.new(:binop, 1) => [sa, sb]
p [sa, sb]
