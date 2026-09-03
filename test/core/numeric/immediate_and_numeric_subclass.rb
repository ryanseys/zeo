# Subclassing immediates and the abstract Numeric (D3).
#
# An immediate subclass (Integer/Float/Symbol/NilClass/TrueClass/FalseClass) is
# a valid DEFINITION but has no instances -- CRuby raises NoMethodError at .new.
# A Numeric subclass is an ordinary object (user <=>/state, Comparable inherited).

class MyInt < Integer
end

puts MyInt.superclass
puts MyInt.ancestors.include?(Integer)
puts MyInt.ancestors.include?(Numeric)
begin
  MyInt.new
rescue => e
  puts "#{e.class}: #{e.message}"
end

class Money < Numeric
  def initialize(cents)
    @cents = cents
  end

  def cents
    @cents
  end

  def <=>(other)
    cents <=> other.cents
  end
end

a = Money.new(500)
b = Money.new(300)
puts a.cents
puts(a > b)
puts(a < b)
puts(a == Money.new(500))
puts a.is_a?(Numeric)
puts a.is_a?(Comparable)
puts a.class
puts [a, b].min.cents
puts [a, b].max.cents
__END__
Integer
true
true
NoMethodError: undefined method 'new' for class MyInt
500
true
false
true
true
true
Money
300
500
