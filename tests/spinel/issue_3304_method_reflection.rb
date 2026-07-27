def dbl(n) = n * 2
def add(a, b) = a + b
p(method(:dbl).box)

c1 = method(:add).curry
raise "FAIL" unless c1[3][4] == 7
raise "FAIL2" unless c1.lambda?

class Calc
  def add(n)
    n + 1
  end
end
p(Calc.instance_method(:add).clone.arity)
um = Calc.instance_method(:add)
c = um.clone
p(c.arity)
d = method(:dbl).dup
p(d.arity)
b = method(:dbl).clone
p(b.box)
puts "ok"
