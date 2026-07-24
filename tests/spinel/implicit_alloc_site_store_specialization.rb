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

rows_a = Store.new
rows_b = Store.new

rows_a.add(RowA.new(1, "alpha"))
rows_b.add(RowB.new(2, "beta"))

puts rows_a.first.name
puts rows_b.first.label
puts rows_a.add(RowA.new(3, "next")).id
puts rows_b.add(RowB.new(4, "again")).id
