# A Struct or Data member lives apart from every instance variable: `@a` in
# the class's own methods (or a subclass's) is a separate ivar, set only by
# assignment or `instance_variable_set`, while the accessors, `to_a`, `to_h`,
# `==`, `dup` and Marshal keep reading the members.
S = Struct.new(:a, :b)
class S
  def peek = @a
  def poke = @a = 99
end
s = S.new(1, 2)
p s.peek
p s.instance_variables
s.poke
p [s.a, s.instance_variables, s.instance_variable_get(:@a), s.to_a, s]
s.a = 5
p [s.a, s.peek, s.to_h]
s.instance_variable_set(:@b, 7)
p [s.b, s.instance_variable_get(:@b), s.instance_variables]

class S3 < S
  def peek_b = @b
end
t = S3.new(3, 4)
p [t.peek_b, t.b, t.to_a, t.instance_variables]

D = Data.define(:x)
class D
  def peek = @x
end
d = D.new(x: 1)
p [d.peek, d.x, d.instance_variables]

K = Struct.new(:k, keyword_init: true)
k = K.new(k: 1)
p [k.k, k.instance_variables, Marshal.load(Marshal.dump(s)).to_a, s == s.dup]
__END__
nil
[]
[1, [:@a], 99, [1, 2], #<struct S a=1, b=2>]
[5, 99, {a: 5, b: 2}]
[2, 7, [:@a, :@b]]
[nil, 4, [3, 4], []]
[nil, 1, []]
[1, [], [5, 2], true]
