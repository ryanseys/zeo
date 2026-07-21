# Left-to-right operand evaluation for ==: the left operand's side effect must
# be visible to the right (Ruby order), not reordered by C's unsequenced
# function arguments.
class Crash
  attr_reader :foo
  def initialize = @foo = []
  def add(list) = list.tap { foo << _1 }
end

obj = Crash.new
raise "tap push/return order" unless obj.add(Crash.new) == obj.foo[0]

# direct (no tap) form
class Box
  attr_reader :items
  def initialize = @items = []
  def push(x)
    items << x
    x
  end
end
b = Box.new
raise "push order" unless b.push(Box.new) == b.items[0]

# right operand carries the effect; left reads the pre-effect state
arr = []
def mutate(a, v) = (a << v; v)
first = arr[0]
res = (arr[0] == mutate(arr, 99))
p res            # nil == 99 -> false in Ruby (arr[0] read before mutate)
p arr.length
puts "ok"
