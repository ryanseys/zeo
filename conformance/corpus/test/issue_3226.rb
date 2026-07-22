D = Data.define(:a, :b) do
  def to_hash = to_h
  def bump = with(b: b + 1)
  def keys = members
  def dk = deconstruct_keys(nil)
  def bad_to_a = (to_a rescue $!.class)
end
d = D.new(a: 1, b: 2)
p d.to_hash
p d.bump.to_h
p d.keys
p d.dk
p d.bad_to_a

S = Struct.new(:a, :b) do
  def pair = to_a
  def sz = size
  def h = to_h
end
s = S.new(1, 2)
p s.pair
p s.sz
p s.h
