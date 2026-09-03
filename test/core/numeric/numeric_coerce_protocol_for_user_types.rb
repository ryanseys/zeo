class Money
  attr_reader :cents
  def initialize(c); @cents = c; end
  def coerce(o); [Money.new(o * 100), self]; end
  def +(o); Money.new(@cents + o.cents); end
  def cents_s; @cents.to_s; end
end
puts((5 + Money.new(250)).cents_s)
puts((2 + 3))
begin; 1 + "x"; rescue TypeError => e; puts e.message; end
class Bad; def coerce(o); 42; end; end
begin; 1 + Bad.new; rescue TypeError; puts "bad"; end
__END__
750
5
String can't be coerced into Integer
bad
