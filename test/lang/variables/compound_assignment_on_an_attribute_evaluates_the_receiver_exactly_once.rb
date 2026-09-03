class Box
  attr_accessor :n
  def initialize
    @n = 0
  end
end
class Tracker
  attr_reader :calls
  def initialize(box)
    @box = box
    @calls = 0
  end
  def get
    @calls += 1
    @box
  end
end

b = Box.new
t = Tracker.new(b)
t.get.n += 5
puts b.n
puts t.calls
__END__
5
1
