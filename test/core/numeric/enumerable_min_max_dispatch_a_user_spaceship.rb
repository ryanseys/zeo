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

temps = [Temp.new(3), Temp.new(9), Temp.new(5)]
puts temps.min.deg
puts temps.max.deg
__END__
3
9
