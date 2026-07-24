RowA = Struct.new(:id, :name)
RowB = Struct.new(:id, :row_a_id, :label)

class HelperStore
  def initialize
    @records = []
    @next_id = 1
  end

  def insert(record)
    record.id = @next_id
    @next_id = @next_id + 1
    @records << record
    record
  end
end

class AStore
  def initialize
    @helper = HelperStore.new
  end

  def add(name)
    @helper.insert(RowA.new(0, name))
  end
end

class BStore
  def initialize
    @helper = HelperStore.new
  end

  def add(row_a_id, label)
    @helper.insert(RowB.new(0, row_a_id, label))
  end
end

a_store = AStore.new
b_store = BStore.new

row_a = a_store.add("alpha")
row_b = b_store.add(row_a.id, "beta")

puts row_a.name
puts row_b.label
puts row_a.id + row_b.row_a_id
