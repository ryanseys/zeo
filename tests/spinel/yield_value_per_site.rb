class A
  def initialize(v); @v = v; end
  def show; "A=#{@v}"; end
end
class B
  def initialize(v); @v = v; end
  def show; "B=#{@v}"; end
end
def fire
  puts yield.show
end
fire { A.new(1) }
fire { B.new(2) }

def twice
  x = yield
  y = yield
  [x, y]
end
p(twice { 3 })
p(twice { "s" }.length)

def use
  v = yield
  v.to_s
end
p use { 7 }
p use { :sym }
p use { A.new(9) }.class.to_s
