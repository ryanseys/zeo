class Item
  attr_accessor :name
end

class Callbacks
  def register(&blk)
    @blk = blk
  end

  def run
    @blk.call
  end
end

c = Callbacks.new
items = []
items << Item.new

c.register do
  puts items.size
end

c.run

it = Item.new
it.name = "one"
items << it

c.run

c2 = Callbacks.new
c2.register do
  puts items.map { |i| i.name.inspect }.join(",")
end
c2.run
