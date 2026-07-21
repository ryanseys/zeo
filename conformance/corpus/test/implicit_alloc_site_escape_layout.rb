RowA = Struct.new(:id, :name)
RowB = Struct.new(:id, :label)

class Store
  def initialize
    @records = []
  end

  def add(record)
    @records << record
    record
  end

  def first
    @records[0]
  end
end

def first_record(store)
  store.first
end

left = Store.new
right = Store.new

left.add(RowA.new(1, "alpha"))
right.add(RowB.new(2, "beta"))

puts first_record(left).name
puts first_record(right).label
