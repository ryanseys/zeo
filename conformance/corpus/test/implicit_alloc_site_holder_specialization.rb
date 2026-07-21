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

holder_a = Holder.new(RowA.new(1, "alpha"))
holder_b = Holder.new(RowB.new(2, "beta"))

puts holder_a.record.name
puts holder_b.record.label
puts holder_a.record.id + holder_b.record.id
