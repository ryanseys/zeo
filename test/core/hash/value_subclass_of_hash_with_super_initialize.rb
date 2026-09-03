# A `Hash` subclass with a `super` in `initialize` (D3): `super(0)` seeds the
# default value on the payload, and inherited `[]`/`[]=`/`size` work.

class Counter < Hash
  def initialize
    super(0)
  end
  def bump(k); self[k] += 1; end
end
c = Counter.new
c.bump(:a); c.bump(:a); c.bump(:b)
puts c[:a]
puts c[:b]
puts c[:missing]
p c
puts c.class
puts c.size
__END__
2
1
0
{a: 2, b: 1}
Counter
2
