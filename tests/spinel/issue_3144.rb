class Crash
  attr_reader :foo

  def initialize = @foo = []
  def add(list) = list.tap { foo << _1 }
end

c = Crash.new
c.add(Crash.new)
c.add(Crash.new)
p c.foo.size
p c.foo.all? { |e| e.is_a?(Crash) }

# explicit block param form
class Box
  attr_reader :items
  def initialize = @items = []
  def push(x) = x.tap { |v| items << v }
end
b = Box.new
b.push("a")
b.push("b")
p b.items
