class Store
  def initialize
    @records = []
  end

  def add(record)
    @records << record
    record
  end

  def count
    @records.length
  end
end

class Other
  def initialize
    @records = []
  end
end

def make_store
  item = Store.new
  item.add(1)
  item
end

def make_other
  item = Other.new
  item
end

def use(store)
  store.add(1)
  store.count
end

left = make_store
right = Store.new
make_other
right.add(2)

puts use(left)
puts use(right)
