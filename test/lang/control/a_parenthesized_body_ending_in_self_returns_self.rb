# `def m = (@x = 2; self)` -- a `Seq` whose tail is `self`. The `Seq`
# block emitted its tail unboxed (`Arc<Concrete>`) while every consumer
# of a `Seq` (which always infers `Poly`) expects a boxed `RubyValue` --
# an E0308 in `Ok(...)` position. The `Seq` now boxes its own tail.

class C
  def initialize = (@x = 1)
  def tap_self = (@x = 2; self)
  attr_reader :x
end
c = C.new
p c.tap_self.equal?(c)
p c.x
__END__
true
2
