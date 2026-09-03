# `refine Target.singleton_class` refines Target's CLASS methods: a covered
# `Target.m` consults the holder, an uncovered one raises as ever, and the
# refined body runs with the class itself as `self`. The Range pair checks a
# refined class method beside an ordinary one on the same holder module, and
# `respond_to?` under the activation answers for the class but not for an
# instance (the singleton candidate matches only Class receivers).
module ClassSide
  refine String.singleton_class do
    def build(n) = "built-#{n}"
  end
  refine Range.singleton_class do
    def from(o) = o.is_a?(Range) ? o : (o..o)
  end
end
begin
  String.build(1)
rescue NoMethodError => e
  p e.class
end
using ClassSide
p String.build(2)
p Range.from(3)
p Range.from(4..5)
p [String.respond_to?(:build), 2.respond_to?(:build)]
__END__
NoMethodError
"built-2"
3..3
4..5
[true, false]
