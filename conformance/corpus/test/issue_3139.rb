class C
  attr_reader :arr, :nums
  def initialize
    @arr = []
    @nums = []
    arr << "a"
    arr << "b"
    nums << 1
  end
end
c = C.new
p c.arr
p c.nums
class D
  attr_accessor :mix
  def initialize
    @mix = []
    mix << 1
    mix << "s"
  end
end
p D.new.mix
