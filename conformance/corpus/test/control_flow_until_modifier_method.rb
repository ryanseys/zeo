class Box
  def initialize
    @v = 0
  end
  def get
    @v
  end
  def bump
    @v = @v + 1
  end
  def at_least?(n)
    get >= n
  end
end
b = Box.new
b.bump until b.at_least?(4)
puts b.get
