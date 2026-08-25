# `Array#uniq` must confirm hash-equal candidates with `eql?`; zeo collapses
# on the user `hash` alone, so two objects with colliding hashes but
# distinct `eql?` dedupe wrongly. Hash keying itself gets this right
# (collision probes confirm), so the miss is uniq's table. (Found by the
# 2026-08-24 probe sweep.)
c = Struct.new(:v) do
  def hash = 0
  def eql?(o) = v == o.v
end
p [c.new(1), c.new(1), c.new(2)].uniq.length
p [c.new(1), c.new(2)].uniq { |x| x }.length
