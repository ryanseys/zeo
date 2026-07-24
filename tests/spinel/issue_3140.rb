class Crash
  def hi = "hi"
end
def fn = Crash.new.tap { yield(_1) }
r = nil
fn { |x| r = x.hi }
p r
# non-escaping tap: typed access
c = Crash.new.tap { |o| p o.hi }
p c.hi
# int tap
n = 5.tap { |x| p x * 2 }
p n
