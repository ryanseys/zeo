# Comparable#between? on a user type (`self >= min && self <= max`) must
# dispatch through the class's <=>, not a raw struct comparison which
# gcc rejects ("invalid operands to binary >="). Direct </>/<=/>= on
# such types already worked via <=>; between? now does too.
class Temp
  include Comparable
  attr_reader :deg
  def initialize(d); @deg = d; end
  def <=>(other); @deg <=> other.deg; end
end

a = Temp.new(10)
puts a.between?(Temp.new(5), Temp.new(15))    # true
puts a.between?(Temp.new(10), Temp.new(15))   # true (inclusive low)
puts a.between?(Temp.new(5), Temp.new(10))    # true (inclusive high)
puts a.between?(Temp.new(11), Temp.new(15))   # false
puts a.between?(Temp.new(1), Temp.new(9))     # false

# Direct comparisons still route through <=>
puts (Temp.new(3) < Temp.new(7))
puts (Temp.new(7) >= Temp.new(7))

# Built-in receivers unaffected
puts 5.between?(1, 10)
puts "m".between?("a", "z")
puts 2.5.between?(1.0, 3.0)
