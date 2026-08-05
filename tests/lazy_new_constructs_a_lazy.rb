# Enumerator::Lazy.new(source) { |yielder, *values| ... } builds a lazy over
# the source with an explicit per-element body. zeo has no `new` row on Lazy
# (NoMethodError); the class is only reachable through #lazy. Belongs to the
# lazy-reparent work tracked in lazy_is_not_an_enumerator.rb.
gen = Enumerator::Lazy.new([1, 2, 3]) { |y, v| y << v * 100 }
puts gen.first(2).inspect
puts gen.class
inf = Enumerator::Lazy.new(1..Float::INFINITY) { |y, v| y << v if v.odd? }
puts inf.first(3).inspect
