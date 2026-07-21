class Obj
  def initialize; @x = 7; end
end
p(!Obj.new)
p(not Obj.new)
o = Obj.new
p(!o)
