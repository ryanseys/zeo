# Comparable#clamp over an exclusive range of user objects raises.
class Ver
  include Comparable
  attr_reader :n
  def initialize(n); @n = n; end
  def <=>(o); n <=> o.n; end
end
p((Ver.new(5).clamp(Ver.new(1)...Ver.new(9)).n rescue $!.class))
p(Ver.new(5).clamp(Ver.new(1)..Ver.new(9)).n)
p(Ver.new(12).clamp(Ver.new(1)..Ver.new(9)).n)
