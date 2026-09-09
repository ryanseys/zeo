# Method#box is nil for a method in no namespace, #curry applies one argument
# at a time, and clone and dup keep the signature.
# (spinel issue #3304)
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
__END__
nil
1
1
1
nil
ok
