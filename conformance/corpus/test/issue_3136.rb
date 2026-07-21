class A
  attr_accessor :b, :i
  def initialize
    @i = 5
  end
  def setb = @b = true
end
a = A.new
p a.b.nil?
p a.i
p a.i.nil?
class C
  attr_accessor :flag
  def initialize
    @flag = false
  end
end
p C.new.flag
p C.new.flag.nil?
