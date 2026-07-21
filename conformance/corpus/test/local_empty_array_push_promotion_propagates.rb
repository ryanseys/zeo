# #450 cascade 3. A local seeded with `[]` (default int_array)
# whose subsequent push observations promote it to an obj-ptr_array:
# the promoted element type needs to propagate back to (a) ivars
# assigned from the local, and (b) the function's return type when
# the local is the body's implicit return. Before the fix, both
# sites pinned at sp_IntArray * based on the local's pre-promotion
# type that scan_writer_calls / infer_body_return saw.

class Item
  attr_accessor :id, :name
  def initialize
    @id = 0
    @name = ""
  end
end

# (a) ivar = local with push promotion.
class Holder
  def initialize
    @items = nil
  end

  def build
    results = []
    a = Item.new
    a.id = 1
    a.name = "first"
    results << a
    b = Item.new
    b.id = 2
    b.name = "second"
    results << b
    @items = results
  end

  def first_name
    @items[0].name
  end
end

h = Holder.new
h.build
puts h.first_name

# (b) method returns the local-pushed-into.
def collect_items
  results = []
  i = 0
  while i < 3
    item = Item.new
    item.id = i
    item.name = "item_" + i.to_s
    results << item
    i = i + 1
  end
  results
end

items = collect_items
puts items.length
puts items[1].name
