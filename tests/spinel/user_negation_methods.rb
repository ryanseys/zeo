class Neg
  def initialize(b); @b = b; end
  def !; !@b; end
  def !=(o); "custom-ne"; end
end
p(!Neg.new(false))
p(Neg.new(true) != Neg.new(true))
