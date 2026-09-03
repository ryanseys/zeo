class Temp
  include Comparable
  attr_reader :deg

  def initialize(d)
    @deg = d
  end

  def <=>(other)
    deg <=> other.deg
  end
end

a = Temp.new(50)
b = Temp.new(70)
puts a < b
puts a > b
puts a <= b
puts b >= a
puts a == Temp.new(50)
puts a.between?(Temp.new(40), Temp.new(60))
puts a.clamp(Temp.new(55), Temp.new(80)).deg
puts a.clamp(Temp.new(20), Temp.new(30)).deg
__END__
true
false
true
true
true
true
55
30
