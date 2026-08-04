# A class whose #coerce is never reached through a numeric operator in this
# program still has to type the chain the parameter feeds. `other` holds
# whatever the numeric protocol hands it, so binding it only where a `3 * obj`
# call site exists left the slot unknown for the whole fixpoint; a late backstop
# lifted it to poly after the factory it feeds had already settled on the
# concrete type, and the build aborted on the argument type.
class U
  attr_reader :v
  def initialize(v); @v = v; end
  def self.scalar(v); new(v); end
  def coerce(other); [U.scalar(other), self]; end
end

u = U.new(Rational(3, 2))
w = u.v
p w
p U.scalar(7).v
p U.scalar(1.5).v
p u.coerce(3)[0].v
p u.coerce(2)[1].v
p u.coerce("s")[0].v

# the same with the factory taking the value straight to the constructor
class V
  attr_reader :v
  def initialize(v); @v = v; end
  def coerce(other); [V.new(other), self]; end
end

p V.new(Complex(1, 2)).v
p V.new(3).coerce(4)[0].v
