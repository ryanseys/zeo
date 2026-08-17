# `5 + obj` runs the numeric coerce protocol from the argument side. The static
# path takes it when the object's class is known at the call site; an operand
# that only reads poly (here from a factory whose ternary widens) reached the
# poly operator and raised "can't be coerced" (#3960).
class F
  attr_reader :n
  def self.from(value)
    value.is_a?(F) ? value : new(value)
  end
  def initialize(n) = @n = n
  def +(other) = F.new(@n + F.from(other).n)
  def -(other) = F.new(@n - F.from(other).n)
  def *(other) = F.new(@n * F.from(other).n)
  def /(other) = F.new(@n / F.from(other).n)
  def coerce(other) = [F.from(other), self]
  def to_s = @n.to_s
end

puts 5 + F.from(3)
puts 5 - F.from(3)
puts 5 * F.from(3)
puts 12 / F.from(3)
puts 2.5 + F.from(3)
puts F.from(5) + 3

# the object on the left keeps its own operator
puts F.from(5) * F.from(3)

# a class with no coerce still raises
class G
  def initialize(n) = @n = n
end
begin
  p(1 + G.new(2))
rescue TypeError => e
  puts e.message
end
