# `undef_method` through an OBJECT's singleton class retires the name for that
# one object. zeo records no per-object tombstone, so the class's definition
# still answers and the call succeeds where ruby raises.
#
# This is why `class << obj; undef :close; end` stays rejected in
# `lower::defs::desugar_singleton_items` rather than becoming the
# `recv.singleton_class.undef_method(:close)` it spells: a compile that
# answered would answer WRONG.
class Foo
  def close = "class"
end

g = Foo.new
g.singleton_class.undef_method(:close)
p g.respond_to?(:close)
p(begin
  g.close
rescue NoMethodError
  :raised
end)

# The retirement is that object's alone.
p Foo.new.close
