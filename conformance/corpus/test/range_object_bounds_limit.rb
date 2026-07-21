# Comparable#clamp against a range of user objects: the bounds fold straight
# into the comparison, so no Range object is built and every inline form works.
# Materializing such a Range is the part that cannot work (sp_Range has mrb_int
# bounds); that is a compile error, covered by docs/limitations.md. #2558
class Ver
  include Comparable
  attr_reader :n
  def initialize(n); @n = n; end
  def <=>(o); n <=> o.n; end
end
lo = Ver.new(1)
hi = Ver.new(9)
p(Ver.new(5).clamp(lo..hi).n)
p(Ver.new(12).clamp(lo..hi).n)
p(Ver.new(0).clamp(lo..hi).n)
p(Ver.new(5).clamp(lo, hi).n)
p(Ver.new(12).clamp(..Ver.new(9)).n)
p(Ver.new(0).clamp(Ver.new(1)..).n)
