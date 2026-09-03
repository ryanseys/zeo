# A compiled `S = Struct.new(:a)` binds through the runtime kernel, so a
# program that patches a core Array or Hash method cannot reach inside the
# constructor. CRuby's `rb_struct_initialize` is C, and reads `argc`.
module PB
  def size = super * 10
  def empty? = false
  def keys = super
end
class Array; prepend PB; end
class Hash; prepend PB; end
p [1, 2].size

S1 = Struct.new(:a, :b)
p S1.new(1, 2).to_a
p S1.new(1).to_a
p S1.new.to_a
p S1.new(a: 1, b: 2).to_a
p S1.new(a: 1).to_a
p(S1.new({ a: 1 }, 2).to_a)
p(begin; S1.new(1, 2, 3); rescue ArgumentError => e; e.message; end)
p(begin; S1.new(z: 9); rescue ArgumentError => e; e.message; end)

D1 = Data.define(:x, :y)
p D1.new(x: 1, y: 2).to_h
p(begin; D1.new(x: 1); rescue ArgumentError => e; e.message; end)
p(begin; D1.new(x: 1, y: 2, z: 3); rescue ArgumentError => e; e.message; end)
p(begin; D1.new(x: 1, y: 2, z: 3, w: 4); rescue ArgumentError => e; e.message; end)

S2 = Struct.new(:a, keyword_init: true)
p(begin; S2.new(z: 1, w: 2); rescue ArgumentError => e; e.message; end)
__END__
20
[1, 2]
[1, nil]
[nil, nil]
[1, 2]
[1, nil]
[{a: 1}, 2]
"struct size differs"
"unknown keywords: z"
{x: 1, y: 2}
"missing keyword: :y"
"unknown keyword: :z"
"unknown keywords: :z, :w"
"unknown keywords: z, w"
