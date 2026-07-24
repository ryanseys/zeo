# A blank-slate receiver has no #send / #public_send -- only __send__ is
# BasicObject's own (#2725). The send desugar leaves such calls unretargeted
# and the blank-slate gate raises CRuby's NoMethodError; __send__ dispatches.
class BO < BasicObject
  def initialize(x = 0); @x = x; end
  def own; @x; end
end
a = BO.new(7)
r = (a.send(:own) rescue $!.class)
p r
r2 = (a.public_send(:own) rescue $!.class)
p r2
p a.__send__(:own)
# ordinary receivers keep the whole surface
class W; def hi(x); "hi:#{x}"; end; end
w = W.new
p w.send(:hi, 1)
p w.send("hi", 2)
p w.__send__(:hi, 3)
p 5.send(:+, 2)
