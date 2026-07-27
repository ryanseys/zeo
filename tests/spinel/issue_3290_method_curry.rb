def add(a, b) = a + b
def add3(a, b, c) = a + b + c
p(method(:add).curry[1][2])
p(method(:add3).curry[1][2][3])
p(method(:add3).curry[10, 20][12])

def cat(a, b) = "#{a}-#{b}"
c = method(:cat).curry
p c["x"]["y"]

class Calc
  def mul(a, b) = a * b
end
p(Calc.new.method(:mul).curry[6][7])
