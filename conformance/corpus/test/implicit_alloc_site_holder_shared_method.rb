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

def unwrap(holder)
  holder.record
end

holder_a = Holder.new(RowA.new(1, "alpha"))
holder_b = Holder.new(RowB.new(2, "beta"))

puts unwrap(holder_a).name
puts unwrap(holder_b).label
puts unwrap(holder_a).id + unwrap(holder_b).id
