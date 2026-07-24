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

def use(store)
  store.add(1)
  store.count
end

left = Store.new
right = Store.new
left.add(10)
right.add(20)

puts use(left)
puts use(right)
