# `Numeric`'s generic implementations -- the ones a user subclass inherits.
# They know nothing but `<=>`, `-`, `/`, `*`, `to_f`, `to_i`, `to_r` and
# `coerce`, so a class that defines only those gets the rest for free. Every
# concrete numeric class carries its own faster row for each, which the
# ancestor walk finds first, so none of this changes what `Integer` answers.

class Deg < Numeric
  attr_reader :v

  def initialize(v) = @v = v
  def <=>(o) = v <=> (o.is_a?(Deg) ? o.v : o)
  def +(o) = Deg.new(v + (o.is_a?(Deg) ? o.v : o))
  def -(o) = Deg.new(v - (o.is_a?(Deg) ? o.v : o))
  def *(o) = Deg.new(v * (o.is_a?(Deg) ? o.v : o))
  def /(o) = Deg.new(v / (o.is_a?(Deg) ? o.v : o))
  def to_f = v.to_f
  def to_i = v.to_i
  def to_r = Rational(v)
  def coerce(o) = [Deg.new(o), self]
  def inspect = "Deg(#{v})"
end

puts "-- sign, through <=> and coerce"
p Deg.new(-7).abs
p Deg.new(-7).magnitude
p Deg.new(7).abs
p(+Deg.new(-7))
p(-Deg.new(-7))
p(-Deg.new(7))

puts "-- floored division and its remainder"
p Deg.new(7).div(Deg.new(2))
p Deg.new(7).modulo(Deg.new(2))
p Deg.new(7) % Deg.new(2)

puts "-- the rounding family, all via Float(self)"
p Deg.new(7.6).round
p Deg.new(7.6).round(1)
p Deg.new(7.6).ceil
p Deg.new(7.6).floor
p Deg.new(7.6).truncate
p Deg.new(-7.6).truncate

puts "-- conversions"
p Deg.new(7).to_int
p Deg.new(7).numerator
p Deg.new(7).denominator
p Deg.new(7).finite?
p Deg.new(7).infinite?

puts "-- the fallback coerce, for a subclass that defines none"
class Plain < Numeric
  def initialize(v) = @v = v
  def to_f = @v.to_f
  def inspect = "Plain"
end
p Plain.new(1).coerce(Plain.new(2))
p Plain.new(1).coerce(3)

puts "-- Kernel#Float goes through to_f for any object that has one"
class Convertible
  def to_f = 1.5
end
p Float(Convertible.new)
p Plain.new(2).floor
begin
  Float(Object.new)
rescue TypeError => e
  p e.message
end

puts "-- Numeric refuses a singleton class"
p Numeric.method_defined?(:singleton_method_added)

puts "-- Complex undefines every one that needs an ordering"
c = Complex(1, 2)
%i[% < <= > >= between? ceil clamp div divmod floor i modulo
   negative? positive? remainder round step truncate].each do |m|
  print c.respond_to?(m) ? "#{m}! " : ""
end
puts "(none)"
p(begin
  c.positive?
rescue NoMethodError
  :no_method_error
end)

puts "-- and the concrete classes still answer for themselves"
p 5.coerce(2.0)
p(-5.abs)
p 7.div(2)
p 7.modulo(2)
p 2.5.round
p 5.numerator, 5.denominator
p Rational(1, 2).positive?
p 5.finite?, (1.0 / 0).infinite?
