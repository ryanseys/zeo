class C001
  attr :x
  def initialize; @x = 5; end
end
p C001.new.x

class Box
  def initialize(v); @v = v; end
end
class BoxPlus < Box; end
Box.class_eval do
  def doubled; @v * 2; end
  define_method(:tripled) { @v * 3 }
  def labelled; @label = "n=#{@v}"; @label; end
end
b = Box.new(21)
p [b.doubled, b.tripled, b.labelled]
p BoxPlus.new(5).doubled

module M; end
M.module_exec { def mm; "mm"; end }
class D; include M; end
p D.new.mm

o = Object.new
o.instance_exec { @invented = 99 }
p o.instance_variable_get(:@invented)
__END__
5
[42, 63, "n=21"]
10
"mm"
99
