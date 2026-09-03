# The message carries the receiver's real `#<Pt:0xADDR @x=1>` inspect
# (address normalized in-program). Semantics oracle-verified: a catchable
# FrozenError, the ivar keeps its old value, readers still work.

class Pt
  def initialize(x)
    @x = x
  end
  def set_x(v)
    @x = v
  end
  def x
    @x
  end
end
p1 = Pt.new(1)
p1.freeze
puts p1.frozen?
begin
  p1.set_x(5)
rescue FrozenError => e
  puts e.send(:message).gsub(/0x[0-9a-f]+/, "0xADDR")
end
puts p1.x
__END__
true
can't modify frozen Pt: #<Pt:0xADDR @x=1>
1
