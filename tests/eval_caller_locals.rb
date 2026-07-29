# A bare `eval` runs in the CALLER's frame, so a dynamic source reads and
# writes the locals around it -- and a local the source introduces stays
# afterwards. The compiler reaches this by materializing the calling scope as
# a Binding at the `eval` site, the same machinery `Kernel#binding` uses.
n = 2
src = "n * 10"

puts "-- reading the caller's locals --"
p eval(src)
p eval("n + 1", nil)

puts "-- writing them --"
eval("n = 40")
p n

puts "-- inside a method, with parameters --"
def scaled(factor, code)
  base = 3
  eval(code)
end
p scaled(4, "base * factor")

puts "-- inside a block --"
[5].each do |item|
  total = 1
  p eval("item + total")
end

puts "-- self, ivars and lexical constants come along --"
class Widget
  SIDES = 4
  def initialize
    @label = "w"
  end

  def describe(code)
    eval(code)
  end
end
w = Widget.new
p w.describe("[@label, SIDES, self.class]")

puts "-- a local the source introduces is the eval's own --"
p eval("introduced = 9; introduced * 2")
p eval("defined?(introduced)")
p defined?(introduced)
