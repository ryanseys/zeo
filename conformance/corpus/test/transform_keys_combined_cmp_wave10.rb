# transform_keys with a mapping AND a fallback block, and Comparable
# operators over a nil-capable user <=> (incomparable raises).
h = { a: 1, b: 2, c: 3 }
p h.transform_keys({ a: :x })
p h.transform_keys({ a: :x }) { |k| k.to_s }
g = { a: 1 }
p g.transform_keys { |k| k.to_s }
class V124
  include Comparable
  attr_reader :n
  def initialize(n); @n = n; end
  def <=>(o)
    return nil unless o.is_a?(V124)
    n <=> o.n
  end
end
a = V124.new(1); b = V124.new(2)
p(a < b)
p(a > b)
p(a.between?(a, b))
r = (begin; a < 5; rescue ArgumentError; "AE"; end); p r
p(a.clamp(a, b).n)
