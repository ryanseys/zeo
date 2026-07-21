RowA = Struct.new(:id, :name)
RowB = Struct.new(:id, :label)

class Holder
  def initialize(record)
    @record = record
  end

  def record
    @record
  end
end

holders = []
holders << Holder.new(RowA.new(1, "alpha"))
holders << Holder.new(RowB.new(2, "beta"))

first = holders[0]
second = holders[1]

puts first.record.name
puts second.record.label
puts first.record.id + second.record.id
